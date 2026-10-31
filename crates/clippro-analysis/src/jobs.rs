//! Analysis scheduler (SPEC §10, §6.3, PLAN P3.3): a worker pool running prepare / section /
//! finish units per job, with checkpoints every `chunk` samples, pause / resume / stop,
//! queue reordering, a large-clip limiter and per-job failure isolation.

use crate::clip::{clip_context, detect_active, effective_rate, finish, measure_section_each, AnalyzeError, AnalyzeOptions};
use clippro_measure::calibration::Calibration;
use clippro_measure::columns::MeasurementSet;
use clippro_measure::image::Rect;
use clippro_measure::sample::{ClipContext, SampleRaw};
use clippro_media::{sample_plan, MediaInfo};
use std::collections::{HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub type JobId = u64;

/// Persistence for checkpoints and results (the project database, or memory in tests).
pub trait RunStore: Send + Sync {
    /// Checkpointed samples per section, in order from the section start.
    fn checkpoints(&self, job: JobId) -> Vec<(u32, Vec<SampleRaw>)>;
    fn save_checkpoint(&self, job: JobId, section: u32, chunk_index: u32, samples: &[SampleRaw]);
    fn active_area(&self, job: JobId) -> Option<Rect>;
    fn save_active_area(&self, job: JobId, area: Rect);
    fn finished(&self, job: JobId, ms: &MeasurementSet);
    fn failed(&self, job: JobId, error: &str);
    /// Audio measurements of the clip (P10), stored before `finished`. Default: ignored.
    fn audio_finished(&self, _job: JobId, _audio: &clippro_audio::AudioSet) {}
}

/// What to analyse.
#[derive(Clone, Debug)]
pub struct JobSpec {
    pub id: JobId,
    pub path: PathBuf,
    pub info: MediaInfo,
    pub seed_base: u64,
    pub opts: AnalyzeOptions,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Progress { job: JobId, done: usize, total: usize },
    Finished { job: JobId },
    Failed { job: JobId, error: String },
    Paused,
    Resumed,
}

#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    pub workers: usize,
    /// Clips longer than this are split into sections.
    pub split_threshold_us: i64,
    pub section_us: i64,
    /// Samples per checkpoint chunk.
    pub chunk: usize,
    /// Max concurrently decoding units on clips at least `large_width` wide.
    pub max_large: usize,
    pub large_width: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        SchedulerConfig {
            workers: cores.div_ceil(2).max(1),
            split_threshold_us: 600_000_000,
            section_us: 300_000_000,
            chunk: 256,
            max_large: 2,
            large_width: 6000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnitState {
    Idle,
    Running,
    Done,
}

struct Section {
    start: usize,
    end: usize,
    measured: Vec<SampleRaw>,
    chunk_index: u32,
    state: UnitState,
    /// The decoder reached the section end (it may deliver fewer samples than planned when
    /// targets lie past the last decodable frame).
    complete: bool,
}

impl Section {
    fn new(start: usize, end: usize) -> Section {
        Section { start, end, measured: Vec::new(), chunk_index: 0, state: UnitState::Idle, complete: false }
    }
    /// Index of the next sample to measure: after the last measured sample.
    fn next(&self) -> usize {
        self.measured.last().map(|r| r.index + 1).unwrap_or(self.start).max(self.start)
    }
}

struct JobRt {
    spec: Arc<JobSpec>,
    plan: Arc<Vec<i64>>,
    ctx: Option<Arc<ClipContext>>,
    prepare: UnitState,
    sections: Vec<Section>,
    finishing: bool,
    status: JobStatus,
    interrupt: Arc<AtomicBool>,
    large: bool,
    last_progress: Instant,
    restored: bool,
}

impl JobRt {
    fn done(&self) -> usize {
        // Finishing takes the measured samples out of the sections; every sample is measured by then.
        if self.finishing {
            return self.plan.len();
        }
        self.sections.iter().map(|s| s.measured.len()).sum()
    }
    fn running_units(&self) -> usize {
        (self.prepare == UnitState::Running) as usize
            + self.sections.iter().filter(|s| s.state == UnitState::Running).count()
            + self.finishing as usize
    }
}

enum Unit {
    Prepare(JobId),
    Section(JobId, usize),
    Finish(JobId),
}

struct State {
    queue: VecDeque<JobId>,
    jobs: HashMap<JobId, JobRt>,
    paused: bool,
    shutdown: bool,
    running: usize,
    large_running: usize,
}

struct Inner {
    cfg: SchedulerConfig,
    cal: Calibration,
    store: Arc<dyn RunStore>,
    events: Mutex<Option<Sender<Event>>>,
    state: Mutex<State>,
    cv: Condvar,
}

pub struct Scheduler {
    inner: Arc<Inner>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Inner {
    fn emit(&self, e: Event) {
        if let Some(tx) = lock(&self.events).as_ref() {
            let _ = tx.send(e);
        }
    }

    /// Picks the next runnable unit in queue order and marks it running.
    fn pick(&self, st: &mut State) -> Option<Unit> {
        if st.paused {
            return None;
        }
        let order: Vec<JobId> = st.queue.iter().copied().collect();
        for id in order {
            let large_full = st.large_running >= self.cfg.max_large;
            let Some(j) = st.jobs.get_mut(&id) else { continue };
            if !matches!(j.status, JobStatus::Queued | JobStatus::Running) {
                continue;
            }
            if j.prepare == UnitState::Idle {
                if j.large && large_full {
                    continue;
                }
                j.prepare = UnitState::Running;
                j.status = JobStatus::Running;
                if j.large {
                    st.large_running += 1;
                }
                st.running += 1;
                return Some(Unit::Prepare(id));
            }
            if j.prepare != UnitState::Done {
                continue;
            }
            if let Some(i) = j.sections.iter().position(|s| s.state == UnitState::Idle && !s.complete && s.next() < s.end) {
                if j.large && large_full {
                    continue;
                }
                j.sections[i].state = UnitState::Running;
                if j.large {
                    st.large_running += 1;
                }
                st.running += 1;
                return Some(Unit::Section(id, i));
            }
            for s in j.sections.iter_mut() {
                if s.state == UnitState::Idle && (s.complete || s.next() >= s.end) {
                    s.state = UnitState::Done;
                }
            }
            if !j.finishing && j.sections.iter().all(|s| s.state == UnitState::Done) {
                j.finishing = true;
                st.running += 1;
                return Some(Unit::Finish(id));
            }
        }
        None
    }

    fn release(&self, st: &mut State, id: JobId, decoding: bool) {
        st.running -= 1;
        if decoding {
            if let Some(j) = st.jobs.get(&id) {
                if j.large {
                    st.large_running -= 1;
                }
            }
        }
    }

    fn fail(&self, st: &mut State, id: JobId, err: String) {
        if let Some(j) = st.jobs.get_mut(&id) {
            if j.status != JobStatus::Failed {
                j.status = JobStatus::Failed;
                j.interrupt.store(true, Ordering::Relaxed);
                self.emit(Event::Failed { job: id, error: err.clone() });
                // Recorded off this thread: the caller holds the scheduler's lock, and the store
                // takes the project's database lock, which other threads hold while asking the
                // scheduler about jobs. Writing it here could deadlock every worker.
                let store = self.store.clone();
                std::thread::spawn(move || store.failed(id, &err));
            }
        }
    }

    fn progress(&self, st: &mut State, id: JobId, force: bool) {
        if let Some(j) = st.jobs.get_mut(&id) {
            if force || j.last_progress.elapsed() >= Duration::from_millis(100) {
                j.last_progress = Instant::now();
                let (done, total) = (j.done(), j.plan.len());
                self.emit(Event::Progress { job: id, done, total });
            }
        }
    }

    fn run_unit(self: &Arc<Self>, unit: Unit) {
        match unit {
            Unit::Prepare(id) => {
                let (spec, plan, restored) = {
                    let mut st = lock(&self.state);
                    let Some(j) = st.jobs.get_mut(&id) else { return };
                    let r = j.restored;
                    j.restored = true;
                    (j.spec.clone(), j.plan.clone(), r)
                };
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<(Rect, Vec<(u32, Vec<SampleRaw>)>), AnalyzeError> {
                    let area = match self.store.active_area(id) {
                        Some(a) => a,
                        None => {
                            let a = detect_active(&spec.path, &spec.info, &plan, &spec.opts)?;
                            self.store.save_active_area(id, a);
                            a
                        }
                    };
                    let cps = if restored { Vec::new() } else { self.store.checkpoints(id) };
                    Ok((area, cps))
                }));
                let mut st = lock(&self.state);
                self.release(&mut st, id, true);
                match res {
                    Ok(Ok((area, cps))) => {
                        if let Some(j) = st.jobs.get_mut(&id) {
                            j.ctx = Some(Arc::new(clip_context(&spec.info, area, spec.seed_base, &spec.opts)));
                            for (sec, samples) in cps {
                                if let Some(s) = j.sections.get_mut(sec as usize) {
                                    if s.measured.is_empty() {
                                        let n = samples.len().min(s.end - s.start);
                                        // Past any existing chunk index (partial chunks make counts irregular).
                                        s.chunk_index = n as u32 + 1;
                                        s.measured = samples.into_iter().take(n).collect();
                                    }
                                }
                            }
                            j.prepare = UnitState::Done;
                        }
                        self.progress(&mut st, id, true);
                    }
                    Ok(Err(e)) => {
                        if let Some(j) = st.jobs.get_mut(&id) {
                            j.prepare = UnitState::Idle;
                        }
                        if !matches!(e, AnalyzeError::Cancelled) {
                            self.fail(&mut st, id, e.to_string());
                        }
                    }
                    Err(_) => self.fail(&mut st, id, "internal error while preparing (panic)".into()),
                }
            }
            Unit::Section(id, si) => {
                let (spec, plan, ctx, next, end, chunk_index, interrupt) = {
                    let st = lock(&self.state);
                    let Some(j) = st.jobs.get(&id) else { return };
                    let s = &j.sections[si];
                    #[allow(clippy::unwrap_used)]
                    let ctx = j.ctx.clone().unwrap();
                    (j.spec.clone(), j.plan.clone(), ctx, s.next(), s.end, s.chunk_index, j.interrupt.clone())
                };
                let chunk = self.cfg.chunk;
                let mut buf: Vec<SampleRaw> = Vec::with_capacity(chunk);
                let mut ci = chunk_index;
                let me = self.clone();
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    measure_section_each(&spec.path, &spec.info, &plan, next, end, &ctx, &spec.opts, &interrupt, |raw| {
                        buf.push(raw);
                        if buf.len() >= chunk {
                            me.store.save_checkpoint(id, si as u32, ci, &buf);
                            ci += 1;
                            let mut st = lock(&me.state);
                            if let Some(j) = st.jobs.get_mut(&id) {
                                j.sections[si].measured.append(&mut buf);
                                j.sections[si].chunk_index = ci;
                            }
                            me.progress(&mut st, id, false);
                        }
                    })
                }));
                // Flush the partial chunk (also on pause/stop) so resume continues from here.
                if !buf.is_empty() {
                    self.store.save_checkpoint(id, si as u32, ci, &buf);
                    ci += 1;
                }
                let mut st = lock(&self.state);
                self.release(&mut st, id, true);
                if let Some(j) = st.jobs.get_mut(&id) {
                    j.sections[si].measured.append(&mut buf);
                    j.sections[si].chunk_index = ci;
                    j.sections[si].state = UnitState::Idle;
                }
                match res {
                    Ok(Ok(())) => {
                        if let Some(j) = st.jobs.get_mut(&id) {
                            j.sections[si].complete = true;
                        }
                        self.progress(&mut st, id, true)
                    }
                    Ok(Err(AnalyzeError::Cancelled)) => self.progress(&mut st, id, true),
                    Ok(Err(e)) => self.fail(&mut st, id, e.to_string()),
                    Err(_) => self.fail(&mut st, id, "internal error while measuring (panic)".into()),
                }
            }
            Unit::Finish(id) => {
                let (spec, ctx, samples) = {
                    let mut st = lock(&self.state);
                    let Some(j) = st.jobs.get_mut(&id) else { return };
                    let samples: Vec<SampleRaw> = j.sections.iter_mut().flat_map(|s| std::mem::take(&mut s.measured)).collect();
                    #[allow(clippy::unwrap_used)]
                    (j.spec.clone(), j.ctx.clone().unwrap(), samples)
                };
                let cal = &self.cal;
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let ms = finish(samples, &ctx, effective_rate(spec.opts.rate, &spec.info), &spec.opts, cal);
                    // Audio (P10): one decode of the audio stream; failures leave the clip without
                    // audio data (the Audio group is then unavailable) rather than failing it.
                    if spec.opts.groups.contains(clippro_types::MetricGroup::Audio) && !spec.info.audio.is_empty() {
                        if let Some(a) = crate::clip::analyze_audio(&spec.path, &spec.info, spec.opts.audio_stream) {
                            self.store.audio_finished(id, &a);
                        }
                    }
                    self.store.finished(id, &ms);
                }));
                let mut st = lock(&self.state);
                self.release(&mut st, id, false);
                match res {
                    Ok(()) => {
                        if let Some(j) = st.jobs.get_mut(&id) {
                            j.status = JobStatus::Done;
                        }
                        st.queue.retain(|q| *q != id);
                        self.emit(Event::Finished { job: id });
                    }
                    Err(_) => self.fail(&mut st, id, "internal error while finishing (panic)".into()),
                }
            }
        }
        self.cv.notify_all();
    }
}

impl Scheduler {
    pub fn new(cfg: SchedulerConfig, cal: Calibration, store: Arc<dyn RunStore>, events: Option<Sender<Event>>) -> Scheduler {
        let workers = cfg.workers.max(1);
        let inner = Arc::new(Inner {
            cfg,
            cal,
            store,
            events: Mutex::new(events),
            state: Mutex::new(State {
                queue: VecDeque::new(),
                jobs: HashMap::new(),
                paused: false,
                shutdown: false,
                running: 0,
                large_running: 0,
            }),
            cv: Condvar::new(),
        });
        let threads = (0..workers)
            .map(|i| {
                let inner = inner.clone();
                std::thread::Builder::new()
                    .name(format!("clippro-analysis-{i}"))
                    .spawn(move || loop {
                        let unit = {
                            let mut st = lock(&inner.state);
                            loop {
                                if st.shutdown {
                                    return;
                                }
                                if let Some(u) = inner.pick(&mut st) {
                                    break u;
                                }
                                st = inner.cv.wait(st).unwrap_or_else(|p| p.into_inner());
                            }
                        };
                        inner.run_unit(unit);
                    })
                    .unwrap_or_else(|e| panic!("spawning analysis worker: {e}"))
            })
            .collect();
        Scheduler { inner, threads }
    }

    /// Adds a job at the back of the queue (or returns early if it's already queued).
    pub fn add(&self, spec: JobSpec) {
        let cfg = &self.inner.cfg;
        let plan = sample_plan(spec.info.duration_us, spec.info.fps(), spec.opts.rate);
        let n = plan.len();
        let mut sections = Vec::new();
        if spec.info.duration_us > cfg.split_threshold_us && cfg.section_us > 0 {
            let mut start = 0;
            let mut boundary = cfg.section_us;
            for k in 0..n {
                if plan[k] >= boundary {
                    sections.push(Section::new(start, k));
                    start = k;
                    boundary += cfg.section_us;
                }
            }
            sections.push(Section::new(start, n));
        } else {
            sections.push(Section::new(0, n));
        }
        let large = spec.info.video.as_ref().map(|v| v.width >= cfg.large_width).unwrap_or(false);
        let id = spec.id;
        let mut st = lock(&self.inner.state);
        if st.jobs.get(&id).map(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running)).unwrap_or(false) {
            return;
        }
        st.jobs.insert(
            id,
            JobRt {
                spec: Arc::new(spec),
                plan: Arc::new(plan),
                ctx: None,
                prepare: UnitState::Idle,
                sections,
                finishing: false,
                status: JobStatus::Queued,
                interrupt: Arc::new(AtomicBool::new(false)),
                large,
                last_progress: Instant::now() - Duration::from_secs(1),
                restored: false,
            },
        );
        st.queue.retain(|q| *q != id);
        st.queue.push_back(id);
        drop(st);
        self.inner.cv.notify_all();
    }

    /// Moves a job to the front of the queue ("Move to next").
    pub fn bump(&self, id: JobId) {
        let mut st = lock(&self.inner.state);
        if st.queue.contains(&id) {
            st.queue.retain(|q| *q != id);
            st.queue.push_front(id);
        }
        drop(st);
        self.inner.cv.notify_all();
    }

    /// Pauses: running units stop at their next sample after checkpointing.
    pub fn pause(&self) {
        let mut st = lock(&self.inner.state);
        st.paused = true;
        for j in st.jobs.values() {
            j.interrupt.store(true, Ordering::Relaxed);
        }
        drop(st);
        self.inner.emit(Event::Paused);
    }

    pub fn resume(&self) {
        let mut st = lock(&self.inner.state);
        st.paused = false;
        for j in st.jobs.values() {
            if j.status != JobStatus::Failed {
                j.interrupt.store(false, Ordering::Relaxed);
            }
        }
        drop(st);
        self.inner.emit(Event::Resumed);
        self.inner.cv.notify_all();
    }

    /// Stops one job; its checkpoints stay, so adding it again resumes.
    pub fn stop(&self, id: JobId) {
        let mut st = lock(&self.inner.state);
        if let Some(j) = st.jobs.get_mut(&id) {
            if matches!(j.status, JobStatus::Queued | JobStatus::Running) {
                j.status = JobStatus::Stopped;
            }
            j.interrupt.store(true, Ordering::Relaxed);
        }
        st.queue.retain(|q| *q != id);
        drop(st);
        self.inner.cv.notify_all();
    }

    pub fn status(&self, id: JobId) -> Option<JobStatus> {
        lock(&self.inner.state).jobs.get(&id).map(|j| j.status)
    }

    pub fn progress(&self, id: JobId) -> Option<(usize, usize)> {
        lock(&self.inner.state).jobs.get(&id).map(|j| (j.done(), j.plan.len()))
    }

    pub fn queue(&self) -> Vec<JobId> {
        lock(&self.inner.state).queue.iter().copied().collect()
    }

    /// True when nothing is running and nothing runnable remains (or paused and drained).
    pub fn is_idle(&self) -> bool {
        let st = lock(&self.inner.state);
        let pending = !st.paused
            && st.queue.iter().any(|id| st.jobs.get(id).map(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running)).unwrap_or(false));
        st.running == 0 && !pending
    }

    /// Blocks until nothing is running and nothing runnable remains (or paused and drained).
    pub fn wait_idle(&self) {
        let mut st = lock(&self.inner.state);
        loop {
            let busy = st.running > 0;
            let pending = !st.paused
                && st.queue.iter().any(|id| st.jobs.get(id).map(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running)).unwrap_or(false));
            if !busy && !pending {
                return;
            }
            st = self.inner.cv.wait_timeout(st, Duration::from_millis(200)).map(|r| r.0).unwrap_or_else(|p| p.into_inner().0);
        }
    }

    /// Units currently running for a job (diagnostics).
    pub fn running_units(&self, id: JobId) -> usize {
        lock(&self.inner.state).jobs.get(&id).map(|j| j.running_units()).unwrap_or(0)
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        {
            let mut st = lock(&self.inner.state);
            st.shutdown = true;
            for j in st.jobs.values() {
                j.interrupt.store(true, Ordering::Relaxed);
            }
        }
        self.inner.cv.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// In-memory `RunStore` (tests, CLI dry runs).
#[derive(Default)]
pub struct MemStore {
    inner: Mutex<MemStoreInner>,
}

#[derive(Default)]
struct MemStoreInner {
    chunks: HashMap<JobId, Vec<(u32, u32, Vec<SampleRaw>)>>,
    active: HashMap<JobId, Rect>,
    results: HashMap<JobId, MeasurementSet>,
    failures: HashMap<JobId, String>,
}

impl MemStore {
    pub fn result(&self, job: JobId) -> Option<MeasurementSet> {
        lock(&self.inner).results.get(&job).cloned()
    }
    pub fn failure(&self, job: JobId) -> Option<String> {
        lock(&self.inner).failures.get(&job).cloned()
    }
    pub fn chunk_count(&self, job: JobId) -> usize {
        lock(&self.inner).chunks.get(&job).map(|v| v.len()).unwrap_or(0)
    }
}

impl RunStore for MemStore {
    fn checkpoints(&self, job: JobId) -> Vec<(u32, Vec<SampleRaw>)> {
        let g = lock(&self.inner);
        let mut v: Vec<(u32, u32, Vec<SampleRaw>)> = g.chunks.get(&job).cloned().unwrap_or_default();
        v.sort_by_key(|c| (c.0, c.1));
        let mut out: Vec<(u32, Vec<SampleRaw>)> = Vec::new();
        for (sec, _, samples) in v {
            match out.last_mut() {
                Some((s, all)) if *s == sec => all.extend(samples),
                _ => out.push((sec, samples)),
            }
        }
        out
    }
    fn save_checkpoint(&self, job: JobId, section: u32, chunk_index: u32, samples: &[SampleRaw]) {
        let mut g = lock(&self.inner);
        let v = g.chunks.entry(job).or_default();
        v.retain(|c| !(c.0 == section && c.1 == chunk_index));
        v.push((section, chunk_index, samples.to_vec()));
    }
    fn active_area(&self, job: JobId) -> Option<Rect> {
        lock(&self.inner).active.get(&job).copied()
    }
    fn save_active_area(&self, job: JobId, area: Rect) {
        lock(&self.inner).active.insert(job, area);
    }
    fn finished(&self, job: JobId, ms: &MeasurementSet) {
        let mut g = lock(&self.inner);
        g.results.insert(job, ms.clone());
        g.chunks.remove(&job);
    }
    fn failed(&self, job: JobId, error: &str) {
        lock(&self.inner).failures.insert(job, error.to_string());
    }
}

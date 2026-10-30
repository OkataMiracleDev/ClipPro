# Command line

`clippro-cli.exe` does everything without the window, for scripts and render farms. Add `--json` to any command for machine-readable output.

| Command | Does |
|---|---|
| `info <file>` | Prints a file's streams, timecode and metadata |
| `analyze <paths…> --project p.clippro` | Imports and analyses; `--rate`, `--groups`, `--workers`, `--cpu-only`, `--no-cache` |
| `score <project> --preset NAME` | Prints the selects; `--set score=55,100` and other overrides |
| `export <project> --format F --out PATH` | Exports; `--layout selects\|with-rejects\|stringout`, `--group-by`, `--separator slug`, `--no-handles`, `--gap 1s` |
| `presets` | Lists the built-in presets |
| `offload <src> --dest D1 --dest D2` | Copies and verifies, with ASC MHL; `--hash xxh64\|xxh3\|xxh128\|md5\|sha1` |
| `proxy <project>` | Makes and links proxies; `--codec prores-proxy\|prores-lt\|dnxhr-lb\|h264`, `--height` |
| `render <project> --out DIR` | Renders the selects; `--copy`, `--nearest-keyframe`, `--codec`, `--template` |
| `bench <files…>` | Measures analysis speed, as a multiple of real time |
| `selftest` | Makes test clips, analyses and exports them, and reports pass or fail |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage error (unknown option or value) |
| 2 | Media error (a file couldn't be read) |
| 3 | Partly done (some clips or copies failed) |
| 4 | Internal error |

## Examples

```
clippro-cli analyze D:\Shoot\Day1 --project D:\Projects\Day1.clippro
clippro-cli export D:\Projects\Day1.clippro --format fcpxml --preset "Talking Head" --out D:\Exports\day1.fcpxml
clippro-cli offload F:\A002R2EC --dest E:\Shuttle --dest G:\RAID
```

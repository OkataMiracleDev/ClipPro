// WebGL2 frame renderer (P6.2/P6.3/P6.5, ADR-002). Takes I420 planes (24-byte `CPF1` header from
// the backend) or any image, converts YUV→RGB with the frame's matrix and range, applies the
// colour transform (a log profile's built-in display transform or a user `.cube`), and draws
// the shader overlays: zebras, false colour and focus peaking.

import type { CubeLut } from "./cube";

export type Overlay = "none" | "zebras" | "false_colour" | "peaking";

export interface DrawOptions {
  overlay: Overlay;
  /** Zebra thresholds as luma fractions (lower band, upper band). */
  zebra: [number, number];
  /** Peaking edge threshold (Sobel magnitude on luma, 0–1). */
  peaking: number;
  /** Stripe phase so zebras crawl. */
  time: number;
  /** Apply the colour transform (off = the log picture as recorded). */
  colour?: boolean;
  /** Before/after split: left of this fraction of the width shows the untransformed picture. */
  split?: number | null;
}

/** Built-in profile transform from the backend: decode LUT and gamut matrix (row-major). */
export interface ProfileColour {
  lut: Float32Array;
  matrix: Float32Array;
}

export interface FrameHeader {
  width: number;
  height: number;
  matrix: number;
  range: number;
  ptsUs: number;
}

export const HEADER_BYTES = 24;

export function parseHeader(buf: ArrayBuffer): FrameHeader | null {
  if (buf.byteLength < HEADER_BYTES) return null;
  const dv = new DataView(buf);
  if (dv.getUint32(0, false) !== 0x43504631) return null; // "CPF1"
  return {
    width: dv.getUint32(4, true),
    height: dv.getUint32(8, true),
    matrix: dv.getUint8(12),
    range: dv.getUint8(13),
    ptsUs: Number(dv.getBigInt64(16, true)),
  };
}

// R = Y + a·Cr, G = Y − b·Cb − c·Cr, B = Y + d·Cb for BT.601, BT.709, BT.2020.
const COEFFS: [number, number, number, number][] = [
  [1.402, 0.344136, 0.714136, 1.772],
  [1.5748, 0.1873, 0.4681, 1.8556],
  [1.4746, 0.16455, 0.57135, 1.8814],
];

/** Column-major mat3 and offset taking raw [Y, U, V] samples (0–1) to RGB. */
export function yuvToRgb(matrix: number, fullRange: boolean): { m: Float32Array; off: Float32Array } {
  const [a, b, c, d] = COEFFS[matrix] ?? COEFFS[1]!;
  const ys = fullRange ? 1 : 255 / 219;
  const cs = fullRange ? 1 : 255 / 224;
  const off = new Float32Array([fullRange ? 0 : 16 / 255, 128 / 255, 128 / 255]);
  // Columns: contribution of Y, of Cb, of Cr.
  const m = new Float32Array([ys, ys, ys, 0, -b * cs, d * cs, a * cs, -c * cs, 0]);
  return { m, off };
}

const VS = `#version 300 es
in vec2 aPos;
out vec2 vUv;
void main() {
  vUv = vec2(aPos.x * 0.5 + 0.5, 0.5 - aPos.y * 0.5);
  gl_Position = vec4(aPos, 0.0, 1.0);
}`;

const FS = `#version 300 es
precision highp float;
precision highp sampler3D;
uniform sampler2D uY, uU, uV, uRGB, uLutP, uLut1;
uniform sampler3D uLut3;
uniform int uColour;
uniform mat3 uGamut;
uniform vec3 uDomMin, uDomMax;
uniform float uLutSize;
uniform bool uFull;
uniform float uSplit;
uniform int uSrc;
uniform mat3 uM;
uniform vec3 uOff;
uniform int uMode;
uniform vec2 uZebra;
uniform float uPeak;
uniform vec2 uTexel;
uniform float uTime;
in vec2 vUv;
out vec4 o;

vec3 rgbAt(vec2 uv) {
  if (uSrc == 1) return texture(uRGB, uv).rgb;
  vec3 yuv = vec3(texture(uY, uv).r, texture(uU, uv).r, texture(uV, uv).r);
  return clamp(uM * (yuv - uOff), 0.0, 1.0);
}
float lumaAt(vec2 uv) { return dot(rgbAt(uv), vec3(0.2126, 0.7152, 0.0722)); }

// Normalised code value (CV/1023), the domain log curves and camera LUTs are defined on.
vec3 toCode(vec3 c) { return uFull ? c : (c * 876.0 + 64.0) / 1023.0; }
// ClipPro display rendering (clippro_score::colour::display).
vec3 displayRender(vec3 lin) {
  vec3 x = max(lin, 0.0) * 0.6;
  vec3 t = (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
  return pow(clamp(t, 0.0, 1.0), vec3(1.0 / 2.4));
}
vec3 colourise(vec3 c) {
  if (uColour == 1) {
    vec3 uv = (toCode(c) * (uLutSize - 1.0) + 0.5) / uLutSize;
    vec3 lin = vec3(texture(uLutP, vec2(uv.r, 0.5)).r, texture(uLutP, vec2(uv.g, 0.5)).r, texture(uLutP, vec2(uv.b, 0.5)).r);
    return displayRender(uGamut * lin);
  }
  if (uColour == 2 || uColour == 3) {
    vec3 d = clamp((toCode(c) - uDomMin) / (uDomMax - uDomMin), 0.0, 1.0);
    vec3 uvw = (d * (uLutSize - 1.0) + 0.5) / uLutSize;
    if (uColour == 2) return texture(uLut3, uvw).rgb;
    return vec3(texture(uLut1, vec2(uvw.r, 0.5)).r, texture(uLut1, vec2(uvw.g, 0.5)).g, texture(uLut1, vec2(uvw.b, 0.5)).b);
  }
  return c;
}

vec3 falseColour(float y) {
  float ire = y * 100.0;
  if (ire < 2.5) return vec3(0.45, 0.0, 0.6);
  if (ire < 10.0) return vec3(0.0, 0.25, 1.0);
  if (ire >= 38.0 && ire < 42.0) return vec3(0.1, 0.85, 0.2);
  if (ire >= 52.0 && ire < 56.0) return vec3(1.0, 0.55, 0.7);
  if (ire >= 97.0 && ire < 99.0) return vec3(1.0, 0.9, 0.0);
  if (ire >= 99.0) return vec3(1.0, 0.1, 0.05);
  return vec3(y * 0.85 + 0.05);
}

void main() {
  vec3 c = rgbAt(vUv);
  bool before = uSplit >= 0.0 && vUv.x < uSplit;
  if (!before) c = colourise(c);
  if (uSplit >= 0.0 && abs(vUv.x - uSplit) < uTexel.x * 1.5) { o = vec4(1.0, 1.0, 1.0, 1.0); return; }
  if (uMode == 1) {
    float y = dot(c, vec3(0.2126, 0.7152, 0.0722));
    float d = gl_FragCoord.x + gl_FragCoord.y + uTime * 24.0;
    if (y >= uZebra.y && mod(d, 8.0) < 4.0) c = vec3(0.0);
    else if (y >= uZebra.x && y < uZebra.y && mod(d, 12.0) < 2.0) c = vec3(1.0);
  } else if (uMode == 2) {
    c = falseColour(dot(c, vec3(0.2126, 0.7152, 0.0722)));
  } else if (uMode == 3) {
    vec2 t = uTexel;
    float tl = lumaAt(vUv + vec2(-t.x, -t.y)), tc = lumaAt(vUv + vec2(0.0, -t.y)), tr = lumaAt(vUv + vec2(t.x, -t.y));
    float ml = lumaAt(vUv + vec2(-t.x, 0.0)), mr = lumaAt(vUv + vec2(t.x, 0.0));
    float bl = lumaAt(vUv + vec2(-t.x, t.y)), bc = lumaAt(vUv + vec2(0.0, t.y)), br = lumaAt(vUv + vec2(t.x, t.y));
    float gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
    float gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);
    float g = length(vec2(gx, gy));
    vec3 dim = c * 0.7;
    c = g > uPeak ? vec3(1.0, 0.2, 0.15) : dim;
  }
  o = vec4(c, 1.0);
}`;

function compile(gl: WebGL2RenderingContext, type: number, src: string) {
  const s = gl.createShader(type)!;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
  return s;
}

export class GlView {
  private gl: WebGL2RenderingContext;
  private prog: WebGLProgram;
  private tex: WebGLTexture[];
  private u: Record<string, WebGLUniformLocation | null> = {};
  private src = 0;
  private w = 0;
  private h = 0;
  private matrix = { m: yuvToRgb(1, false).m, off: yuvToRgb(1, false).off };
  private full = false;
  /** 0 none, 1 profile, 2 cube 3D, 3 cube 1D. */
  private colourMode = 0;
  private lutSize = 2;
  private gamut: Float32Array = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]);
  private dom: [Float32Array, Float32Array] = [new Float32Array([0, 0, 0]), new Float32Array([1, 1, 1])];
  private profile: ProfileColour | null = null;
  private cube: CubeLut | null = null;

  constructor(readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: false, premultipliedAlpha: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error("WebGL2 unavailable");
    this.gl = gl;
    const p = gl.createProgram()!;
    gl.attachShader(p, compile(gl, gl.VERTEX_SHADER, VS));
    gl.attachShader(p, compile(gl, gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? "link");
    this.prog = p;
    gl.useProgram(p);
    for (const n of ["uY", "uU", "uV", "uRGB", "uLutP", "uLut1", "uLut3", "uColour", "uGamut", "uDomMin", "uDomMax", "uLutSize", "uFull", "uSplit", "uSrc", "uM", "uOff", "uMode", "uZebra", "uPeak", "uTexel", "uTime"])
      this.u[n] = gl.getUniformLocation(p, n);
    const vb = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, vb);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(p, "aPos");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    // Units: 0–2 Y/U/V, 3 RGB image, 4 profile LUT, 5 cube 1D, 6 cube 3D.
    this.tex = [0, 1, 2, 3, 4, 5, 6].map((i) => {
      const target = i === 6 ? gl.TEXTURE_3D : gl.TEXTURE_2D;
      const t = gl.createTexture()!;
      gl.activeTexture(gl.TEXTURE0 + i);
      gl.bindTexture(target, t);
      gl.texParameteri(target, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(target, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(target, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(target, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      if (i === 6) gl.texParameteri(target, gl.TEXTURE_WRAP_R, gl.CLAMP_TO_EDGE);
      return t;
    });
    ["uY", "uU", "uV", "uRGB", "uLutP", "uLut1", "uLut3"].forEach((n, i) => gl.uniform1i(this.u[n]!, i));
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  /** Uploads I420 planes after a `CPF1` header. Returns the header, or null if malformed. */
  setI420(buf: ArrayBuffer): FrameHeader | null {
    const hd = parseHeader(buf);
    if (!hd) return null;
    const { width: w, height: h } = hd;
    const c = (w / 2) * (h / 2);
    if (buf.byteLength < HEADER_BYTES + w * h + 2 * c) return null;
    const gl = this.gl;
    const planes = [new Uint8Array(buf, HEADER_BYTES, w * h), new Uint8Array(buf, HEADER_BYTES + w * h, c), new Uint8Array(buf, HEADER_BYTES + w * h + c, c)];
    planes.forEach((plane, i) => {
      gl.activeTexture(gl.TEXTURE0 + i);
      gl.bindTexture(gl.TEXTURE_2D, this.tex[i]!);
      const [pw, ph] = i === 0 ? [w, h] : [w / 2, h / 2];
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, pw, ph, 0, gl.RED, gl.UNSIGNED_BYTE, plane);
    });
    this.src = 0;
    this.w = w;
    this.h = h;
    this.matrix = yuvToRgb(hd.matrix, hd.range === 1);
    this.full = hd.range === 1;
    return hd;
  }

  /** The clip's built-in profile transform (null for Rec.709 or none). A user cube wins. */
  setProfile(p: ProfileColour | null) {
    this.profile = p;
    if (p) {
      const gl = this.gl;
      gl.activeTexture(gl.TEXTURE4);
      gl.bindTexture(gl.TEXTURE_2D, this.tex[4]!);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R16F, p.lut.length, 1, 0, gl.RED, gl.FLOAT, p.lut);
    }
    this.pickColour();
  }

  /** A user `.cube` LUT (null to remove). Applied to code values, output is display RGB. */
  setCube(l: CubeLut | null) {
    this.cube = l;
    if (l) {
      const gl = this.gl;
      if (l.kind === "3d") {
        gl.activeTexture(gl.TEXTURE6);
        gl.bindTexture(gl.TEXTURE_3D, this.tex[6]!);
        gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGBA16F, l.size, l.size, l.size, 0, gl.RGBA, gl.FLOAT, l.data);
      } else {
        gl.activeTexture(gl.TEXTURE5);
        gl.bindTexture(gl.TEXTURE_2D, this.tex[5]!);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, l.size, 1, 0, gl.RGBA, gl.FLOAT, l.data);
      }
    }
    this.pickColour();
  }

  private pickColour() {
    if (this.cube) {
      this.colourMode = this.cube.kind === "3d" ? 2 : 3;
      this.lutSize = this.cube.size;
      this.dom = [Float32Array.from(this.cube.domainMin), Float32Array.from(this.cube.domainMax)];
    } else if (this.profile) {
      this.colourMode = 1;
      this.lutSize = this.profile.lut.length;
      this.gamut = this.profile.matrix;
    } else {
      this.colourMode = 0;
    }
  }

  setImage(img: TexImageSource, w: number, h: number) {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE3);
    gl.bindTexture(gl.TEXTURE_2D, this.tex[3]!);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
    this.src = 1;
    this.w = w;
    this.h = h;
  }

  get hasFrame() {
    return this.w > 0;
  }

  draw(o: DrawOptions) {
    if (!this.hasFrame) return;
    const gl = this.gl;
    const c = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const cw = Math.max(1, Math.round(c.clientWidth * dpr));
    const ch = Math.max(1, Math.round(c.clientHeight * dpr));
    if (c.width !== cw || c.height !== ch) {
      c.width = cw;
      c.height = ch;
    }
    gl.viewport(0, 0, cw, ch);
    gl.useProgram(this.prog);
    gl.uniform1i(this.u.uSrc!, this.src);
    gl.uniformMatrix3fv(this.u.uM!, false, this.matrix.m);
    gl.uniform3fv(this.u.uOff!, this.matrix.off);
    gl.uniform1i(this.u.uMode!, { none: 0, zebras: 1, false_colour: 2, peaking: 3 }[o.overlay]);
    gl.uniform2f(this.u.uZebra!, o.zebra[0], o.zebra[1]);
    gl.uniform1f(this.u.uPeak!, o.peaking);
    gl.uniform2f(this.u.uTexel!, 1 / this.w, 1 / this.h);
    gl.uniform1f(this.u.uTime!, o.time);
    // Images (browser preview) are already display-referred: no colour transform.
    const colour = o.colour !== false && this.src === 0 ? this.colourMode : 0;
    gl.uniform1i(this.u.uColour!, colour);
    gl.uniformMatrix3fv(this.u.uGamut!, true, this.gamut);
    gl.uniform3fv(this.u.uDomMin!, this.dom[0]);
    gl.uniform3fv(this.u.uDomMax!, this.dom[1]);
    gl.uniform1f(this.u.uLutSize!, this.lutSize);
    gl.uniform1i(this.u.uFull!, this.full ? 1 : 0);
    gl.uniform1f(this.u.uSplit!, colour && o.split !== null && o.split !== undefined ? o.split : -1);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }
}

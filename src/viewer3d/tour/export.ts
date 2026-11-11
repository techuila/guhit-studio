// Tour video export (lane L9): renders a tour's frames in the window
// (render/tourJob.ts), encodes them and streams the file through
// `video_begin`, `video_append` and `video_finish`, which saves it to
// Visuals (DECISIONS D54). Answers the `export_tour` window task
// (shell/windowTasks.ts) and the Tours panel's Export button.
//
// Encoding, first that works:
// 1. WebCodecs H.264 ("avc"), muxed into MP4 by Mediabunny;
// 2. VP9 in MP4, when `VideoEncoder.isConfigSupported` says no to H.264
//    (Windows N editions, an older WebKit);
// 3. a folder of PNG frames, when there is no WebCodecs at all, and the
//    result says so.
// The MP4 is fragmented (moof and mdat per second), so every byte is
// written in order: chunks of 2 MiB stream to Rust as they come, at most
// four waiting, and the file is never one big data URL.
//
// A hidden window slows its timers to about once a second (WKWebView most
// of all), so the export warns when the window is hidden.
//
// The frames draw from the job's own camera, so the live view is never moved
// by the export and nothing moving it reaches the video. While it runs,
// presets, saved views and agents' looks leave the view where it is
// (`viewHeld`), and the view takes no refine samples (`holdRefine`), which
// would restart after every frame and take the GPU from it (GR3 P1-1, P2).

import { EncodedPacket, EncodedVideoPacketSource, Mp4OutputFormat, Output, StreamTarget, type StreamTargetChunk } from "mediabunny";
import type { IpcError, RenderSize, VideoInfo, WindowReply } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { showInFolder } from "../../shell/reveal";
import { useApp } from "../../state/store";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { TourJob, TOUR_SAMPLES } from "../render/tourJob";
import { useLiveView } from "../render/liveView";
import { useViewer } from "../viewerStore";
import { tourFrames, type TourFrames } from "./frames";
import { hideTourPath } from "./pathPreview";
import { stopTour } from "./player";
import { useTours } from "./tourStore";

export type TourCodec = "avc" | "vp9" | "png";

/** Pixel size of each render size. */
export const SIZES: Record<RenderSize, { width: number; height: number }> = {
  hd: { width: 1920, height: 1080 },
  qhd: { width: 2560, height: 1440 },
  "4k": { width: 3840, height: 2160 },
  square: { width: 2048, height: 2048 },
};

/** Bytes per `video_append`, before base64; the backend takes at most 4 MiB. */
export const CHUNK_BYTES = 2 * 1024 * 1024;
/** Chunks waiting to be written, at most. */
export const MAX_QUEUE = 4;

function fail(code: string, message: string): never {
  throw { code, message, element_ids: [] } satisfies IpcError;
}

/** Bits per second for a size and frame rate: about 0.2 bits a pixel. */
export function bitrateFor(width: number, height: number, fps: number): number {
  return Math.round(Math.min(Math.max(width * height * fps * 0.2, 4e6), 60e6));
}

/** H.264 High profile at the lowest level that fits, then the levels above it. */
export function avcCodecs(width: number, height: number, fps: number): string[] {
  const rate = width * height * fps;
  const levels = rate <= 1920 * 1088 * 30 ? ["28", "2a", "32", "33", "34"] : rate <= 1920 * 1088 * 60 ? ["2a", "32", "33", "34"] : rate <= 2560 * 1440 * 30 ? ["32", "33", "34"] : rate <= 3840 * 2160 * 30 ? ["33", "34"] : ["34"];
  return [...levels.map((l) => `avc1.6400${l}`), ...levels.map((l) => `avc1.4d00${l}`)];
}

/** VP9 profile 0, 8 bit, at a level for the size. */
export function vp9Codecs(width: number, height: number): string[] {
  const px = width * height;
  const levels = px <= 1920 * 1080 ? ["41", "50", "51"] : px <= 2560 * 1440 ? ["50", "51"] : ["51", "60"];
  return levels.map((l) => `vp09.00.${l}.08`);
}

interface Chosen {
  codec: TourCodec;
  config: VideoEncoderConfig | null;
}

/**
 * True when an encoder set up with `config` turns three frames into three
 * chunks within `timeoutMs`. Some engines say yes to a config and then never
 * answer: Safari 27's H.264 Main and High in "quality" latency mode take
 * frames and never give them back, so a yes from `isConfigSupported` is not
 * enough. Without a DOM (tests) it is true.
 */
export async function encoderWorks(config: VideoEncoderConfig, timeoutMs = 4000): Promise<boolean> {
  if (typeof document === "undefined" || typeof VideoFrame === "undefined") return true;
  const canvas = document.createElement("canvas");
  canvas.width = config.width;
  canvas.height = config.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return true;
  let out = 0;
  let failed = false;
  let enc: VideoEncoder | null = null;
  try {
    enc = new VideoEncoder({
      output: () => {
        out++;
      },
      error: () => {
        failed = true;
      },
    });
    enc.configure(config);
    for (let i = 0; i < 3; i++) {
      ctx.fillStyle = `hsl(${i * 90}, 50%, 50%)`;
      ctx.fillRect(0, 0, config.width, config.height);
      const frame = new VideoFrame(canvas, { timestamp: i * 33333, duration: 33333 });
      enc.encode(frame, { keyFrame: i === 0 });
      frame.close();
    }
    const flushed = await Promise.race([
      enc.flush().then(
        () => true,
        () => false,
      ),
      new Promise<boolean>((r) => setTimeout(() => r(false), timeoutMs)),
    ]);
    return flushed && !failed && out >= 3;
  } catch {
    return false;
  } finally {
    try {
      if (enc && enc.state !== "closed") enc.close();
    } catch {
      // Already closed by an error.
    }
  }
}

/** The first of these configs this browser says it supports, as it answers it. */
async function firstSupported(configs: VideoEncoderConfig[]): Promise<VideoEncoderConfig | null> {
  for (const config of configs) {
    try {
      const r = await VideoEncoder.isConfigSupported(config);
      if (r.supported) return r.config ?? config;
    } catch {
      // A malformed or unknown string: try the next.
    }
  }
  return null;
}

/**
 * The encoder: H.264 when this browser can, VP9 in MP4 when it says no,
 * PNG frames without WebCodecs. Each family (H.264 High, Main, Baseline,
 * then VP9) is tried at the lowest level the browser supports, in "quality"
 * latency mode and then "realtime" (no reordered frames), and must encode a
 * few frames (`works`) before it is used. `force` starts lower down.
 */
export async function chooseEncoder(
  width: number,
  height: number,
  fps: number,
  force: TourCodec | null = null,
  works: (config: VideoEncoderConfig) => Promise<boolean> = encoderWorks,
): Promise<Chosen> {
  if (force === "png" || typeof VideoEncoder === "undefined") return { codec: "png", config: null };
  const base = { width, height, framerate: fps, bitrate: bitrateFor(width, height, fps) };
  const avc = (codecs: string[]) => codecs.map((codec) => ({ ...base, codec, avc: { format: "avc" as const } }));
  const families: Array<[TourCodec, VideoEncoderConfig[]]> = [];
  if (force !== "vp9") {
    const high = avcCodecs(width, height, fps).filter((c) => c.startsWith("avc1.64"));
    families.push(["avc", avc(high)]);
    families.push(["avc", avc(high.map((c) => c.replace("avc1.64", "avc1.4d")))]);
    families.push(["avc", avc(high.map((c) => c.replace("avc1.64", "avc1.42")))]);
  }
  families.push(["vp9", vp9Codecs(width, height).map((codec) => ({ ...base, codec }))]);
  for (const [codec, configs] of families) {
    const config = await firstSupported(configs.map((c) => ({ ...c, latencyMode: "quality" as const })));
    if (!config) continue;
    if (await works(config)) return { codec, config };
    const realtime: VideoEncoderConfig = { ...config, latencyMode: "realtime" };
    if (await works(realtime)) return { codec, config: realtime };
  }
  return { codec: "png", config: null };
}

function toBase64(bytes: Uint8Array): string {
  const native = (bytes as Uint8Array & { toBase64?: () => string }).toBase64;
  if (typeof native === "function") return native.call(bytes);
  let out = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) out += String.fromCharCode(...bytes.subarray(i, i + step));
  return btoa(out);
}

/** Writes bytes to the upload in order, at most MAX_QUEUE chunks waiting. */
class Uploader {
  written = 0;
  private chain: Promise<void> = Promise.resolve();
  private waiting = 0;
  private error: unknown = null;

  constructor(private id: string) {}

  async write(data: Uint8Array): Promise<void> {
    if (this.error) throw this.error;
    for (let at = 0; at < data.length; at += CHUNK_BYTES) {
      const piece = data.slice(at, Math.min(at + CHUNK_BYTES, data.length));
      while (this.waiting >= MAX_QUEUE) await this.chain;
      this.waiting++;
      this.chain = this.chain.then(async () => {
        try {
          if (!this.error) await ipc.videoAppend(this.id, toBase64(piece));
        } catch (e) {
          this.error = e;
        } finally {
          this.waiting--;
        }
      });
      this.written += piece.length;
    }
  }

  async flush(): Promise<void> {
    await this.chain;
    if (this.error) throw this.error;
  }
}

/** What an export makes. */
export interface ExportOptions {
  codec?: TourCodec | null;
  samples?: number;
}

/** Waits until the 3D view is mounted (the plan-only view becomes split). */
async function engineReady() {
  const app = useApp.getState();
  if (!useLiveView.getState().engine && app.viewMode === "2d") app.setViewMode("split");
  const start = Date.now();
  while (!useLiveView.getState().engine) {
    if (Date.now() - start > 8000) fail("invalid", "The 3D view in Guhit Studio did not come up. Open it and try again.");
    await new Promise((r) => setTimeout(r, 60));
  }
  if (useViewer.getState().nav !== "orbit") {
    useViewer.getState().setNav("orbit");
    await new Promise((r) => setTimeout(r, 450));
  }
  return useLiveView.getState().engine!;
}

function loadImage(url: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = url;
  });
}

const CODEC_NAMES: Record<TourCodec, string> = { avc: "H.264", vp9: "VP9", png: "PNG frames" };

/**
 * Exports tour `tourId` at `size` to Visuals. One export at a time. The
 * reply's `render_ids` holds the video record and `image` its poster.
 */
export async function exportTour(tourId: string, size: RenderSize, opts: ExportOptions = {}): Promise<WindowReply> {
  const tours = useTours.getState();
  if (tours.exporting) fail("invalid", "A tour video is exporting already. Wait for it, or stop it in the Tours panel.");
  const doc = useApp.getState().doc;
  const tour = doc?.project.elements.find((e) => e.id === tourId);
  if (!doc || !tour || tour.kind !== "tour") fail("not_found", `No tour has id "${tourId}".`);
  const { width, height } = SIZES[size] ?? SIZES.hd;
  const dev = (globalThis as { __guhitTourCodec?: TourCodec }).__guhitTourCodec ?? null;
  const force = opts.codec ?? dev;
  tours.setExport({ tourId, size, done: 0, total: 0, codec: null, phase: "Preparing", cancelled: false });
  stopTour();
  const showPath = hideTourPath();
  const warnHidden = () => {
    if (document.visibilityState === "hidden") {
      useApp.getState().toast("info", "Keep Guhit Studio on screen while the video exports: a hidden window slows it right down.");
    }
  };
  document.addEventListener("visibilitychange", warnHidden);
  warnHidden();
  let uploadId: string | null = null;
  let job: TourJob | null = null;
  let engineRef: ViewerEngine | null = null;
  let releaseRefine: (() => void) | null = null;
  const started = performance.now();
  try {
    const engine = await engineReady();
    engineRef = engine;
    releaseRefine = engine.holdRefine();
    const revision = doc.revision;
    const data: TourFrames = await tourFrames(tourId);
    if (data.frame_count === 0) fail("invalid", "This tour has no shots to export yet.");
    const chosen = await chooseEncoder(width, height, data.fps, force);
    useTours.getState().patchExport({ total: data.frame_count, codec: chosen.codec, phase: "Rendering" });
    job = new TourJob(engine, data, width, height, opts.samples ?? TOUR_SAMPLES);
    if (data.frames.some((f) => f.fx?.kind === "plan_lift")) {
      const capture = useApp.getState().capturePlan;
      const lowest = [...doc.project.levels].sort((a, b) => a.elevation_mm - b.elevation_mm)[0];
      if (capture) job.setPlan(await loadImage(await capture(lowest?.id ?? null).catch(() => "")));
    }
    uploadId = (await ipc.videoBegin(tourId, revision)).id;
    const upload = new Uploader(uploadId);
    const posterAt = Math.min(data.frame_count - 1, Math.round(data.frame_count * 0.3));
    let poster = "";
    const cancelled = () => useTours.getState().exporting?.cancelled === true;

    // Packets out of the encoder: a "realtime" encoder may drop a frame.
    let packets = data.frame_count;
    if (chosen.codec === "png") {
      for (let i = 0; i < data.frame_count; i++) {
        if (cancelled()) fail("cancelled", "The export was stopped.");
        const canvas = await job.frame(i);
        if (i === posterAt) poster = canvas.toDataURL("image/jpeg", 0.88);
        const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/png"));
        if (!blob) fail("invalid", "A frame could not be saved as a PNG.");
        await upload.write(new Uint8Array(await blob.arrayBuffer()));
        useTours.getState().patchExport({ done: i + 1 });
      }
    } else {
      const config = chosen.config!;
      packets = 0;
      const source = new EncodedVideoPacketSource(chosen.codec);
      const sink = new WritableStream<StreamTargetChunk>(
        {
          write: async (chunk) => {
            // Fragmented MP4 is written in order; anything else is a bug here.
            if (chunk.position !== upload.written) throw new Error(`The video writer jumped to byte ${chunk.position} at ${upload.written}.`);
            await upload.write(chunk.data);
          },
        },
        new CountQueuingStrategy({ highWaterMark: MAX_QUEUE }),
      );
      const output = new Output({
        format: new Mp4OutputFormat({ fastStart: "fragmented", minimumFragmentDuration: 1 }),
        target: new StreamTarget(sink, { chunked: true, chunkSize: CHUNK_BYTES }),
      });
      output.addVideoTrack(source, { frameRate: data.fps });
      await output.start();
      let muxing: Promise<void> = Promise.resolve();
      let pending = 0;
      let encodeError: unknown = null;
      const encoder = new VideoEncoder({
        output: (chunk, meta) => {
          pending++;
          packets++;
          const packet = EncodedPacket.fromEncodedChunk(chunk);
          muxing = muxing
            .then(() => source.add(packet, meta ?? undefined))
            .catch((e) => {
              encodeError ??= e;
            })
            .finally(() => {
              pending--;
            });
        },
        error: (e) => {
          encodeError ??= e;
        },
      });
      encoder.configure(config);
      const step = Math.round(1e6 / data.fps);
      try {
        for (let i = 0; i < data.frame_count; i++) {
          if (cancelled()) fail("cancelled", "The export was stopped.");
          if (encodeError) throw encodeError;
          const canvas = await job.frame(i);
          if (i === posterAt) poster = canvas.toDataURL("image/jpeg", 0.88);
          const frame = new VideoFrame(canvas, { timestamp: i * step, duration: step });
          encoder.encode(frame, { keyFrame: i % (data.fps * 2) === 0 });
          frame.close();
          // Keep the encoder and the muxer short of work: under 8 waiting.
          while (encoder.encodeQueueSize > MAX_QUEUE || pending > MAX_QUEUE) {
            await Promise.race([new Promise((r) => encoder.addEventListener("dequeue", r, { once: true })), muxing, new Promise((r) => setTimeout(r, 50))]);
          }
          useTours.getState().patchExport({ done: i + 1 });
        }
        useTours.getState().patchExport({ phase: "Saving" });
        // An encoder that stops answering must not hold the export for good.
        const flushed = await Promise.race([encoder.flush().then(() => true), new Promise<boolean>((r) => setTimeout(() => r(false), 60_000))]);
        if (!flushed) fail("invalid", "The video encoder stopped answering. Try again, or export at a smaller size.");
        await muxing;
        if (encodeError) throw encodeError;
        source.close();
        await output.finalize();
      } finally {
        if (encoder.state !== "closed") encoder.close();
      }
    }
    await upload.flush();
    const info: VideoInfo = { path: "", tour_id: tourId, seconds: data.frame_count / data.fps, fps: data.fps, width, height, codec: chosen.codec };
    const record = await ipc.videoFinish(uploadId, poster, info);
    uploadId = null;
    useViewer.getState().bumpRenders();
    const secs = Math.round((performance.now() - started) / 100) / 10;
    const why = force === chosen.codec ? " (as asked)" : chosen.codec === "png" ? ": this computer has no video encoder for MP4" : chosen.codec === "vp9" ? ": this computer has no H.264 encoder" : "";
    const what = chosen.codec === "png" ? `a folder of PNG frames${why}` : `an MP4 (${CODEC_NAMES[chosen.codec]})${why}`;
    const dropped = data.frame_count - packets;
    const lost = dropped > 0 ? ` The encoder dropped ${dropped} frame${dropped === 1 ? "" : "s"}.` : "";
    const note = `Exported "${data.name}": ${info.seconds.toFixed(1)} s, ${data.frame_count} frames at ${width} x ${height}, as ${what}, in ${secs} s. Saved to Visuals.${lost}`;
    useApp.getState().toast(chosen.codec === "png" ? "info" : "success", note, showInFolder(record.video?.path));
    return { render_ids: [record.id], image: poster || null, note };
  } catch (e) {
    if (uploadId) await ipc.videoAbort(uploadId).catch(() => null);
    throw e;
  } finally {
    job?.dispose();
    document.removeEventListener("visibilitychange", warnHidden);
    showPath();
    // The view is where it was: only the refine starts again.
    releaseRefine?.();
    engineRef?.invalidate();
    useTours.getState().setExport(null);
  }
}

import { afterEach, describe, expect, it, vi } from "vitest";
import { avcCodecs, bitrateFor, chooseEncoder, CHUNK_BYTES, MAX_QUEUE, vp9Codecs } from "./export";

afterEach(() => {
  vi.unstubAllGlobals();
});

/** A VideoEncoder that supports only the codecs whose string starts with one of `prefixes`. */
function encoderFor(prefixes: string[]) {
  const asked: string[] = [];
  vi.stubGlobal("VideoEncoder", {
    isConfigSupported: async (config: VideoEncoderConfig) => {
      asked.push(config.codec);
      return { supported: prefixes.some((p) => config.codec.startsWith(p)), config };
    },
  });
  return asked;
}

describe("tour video encoding", () => {
  it("asks for H.264 at a level that fits the size, High profile first", () => {
    expect(avcCodecs(1920, 1080, 30)[0]).toBe("avc1.640028");
    expect(avcCodecs(1920, 1080, 60)[0]).toBe("avc1.64002a");
    expect(avcCodecs(3840, 2160, 30)[0]).toBe("avc1.640033");
    expect(vp9Codecs(1920, 1080)[0]).toBe("vp09.00.41.08");
    expect(bitrateFor(1920, 1080, 30)).toBe(Math.round(1920 * 1080 * 30 * 0.2));
  });

  it("uses H.264 when it can, VP9 in MP4 when it says no, PNG frames without either", async () => {
    encoderFor(["avc1"]);
    expect((await chooseEncoder(1920, 1080, 30)).codec).toBe("avc");
    const asked = encoderFor(["vp09"]);
    const vp9 = await chooseEncoder(1920, 1080, 30);
    expect(vp9.codec).toBe("vp9");
    // Every H.264 level of High, Main and Baseline was asked about first.
    const high = avcCodecs(1920, 1080, 30).filter((c) => c.startsWith("avc1.64")).length;
    expect(asked.filter((c) => c.startsWith("avc1")).length).toBe(high * 3);
    encoderFor([]);
    expect((await chooseEncoder(1920, 1080, 30)).codec).toBe("png");
    vi.unstubAllGlobals();
    vi.stubGlobal("VideoEncoder", undefined);
    expect((await chooseEncoder(1920, 1080, 30)).codec).toBe("png");
  });

  it("passes over a config that says yes and then never encodes", async () => {
    // Safari 27: H.264 Main and High in "quality" latency mode take frames and never answer.
    encoderFor(["avc1", "vp09"]);
    const tried: string[] = [];
    const safari = async (c: VideoEncoderConfig) => {
      tried.push(`${c.codec} ${c.latencyMode}`);
      return c.latencyMode === "realtime" || c.codec.startsWith("avc1.42");
    };
    const chosen = await chooseEncoder(1920, 1080, 30, null, safari);
    expect(chosen.codec).toBe("avc");
    expect(chosen.config?.codec).toBe("avc1.640028");
    expect(chosen.config?.latencyMode).toBe("realtime");
    expect(tried).toEqual(["avc1.640028 quality", "avc1.640028 realtime"]);
    // No H.264 encodes at all: VP9, then frames.
    const noAvc = async (c: VideoEncoderConfig) => c.codec.startsWith("vp09");
    expect((await chooseEncoder(1920, 1080, 30, null, noAvc)).codec).toBe("vp9");
    expect((await chooseEncoder(1920, 1080, 30, null, async () => false)).codec).toBe("png");
  });

  it("can be forced down to a fallback", async () => {
    encoderFor(["avc1", "vp09"]);
    expect((await chooseEncoder(1920, 1080, 30, "vp9")).codec).toBe("vp9");
    expect((await chooseEncoder(1920, 1080, 30, "png")).codec).toBe("png");
  });

  it("streams in chunks the backend takes, a short queue at a time", () => {
    // video_append takes at most 4 MiB decoded.
    expect(CHUNK_BYTES).toBeLessThanOrEqual(4 * 1024 * 1024);
    expect(MAX_QUEUE).toBeLessThan(8);
  });
});

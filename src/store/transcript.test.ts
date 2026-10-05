import { beforeEach, describe, expect, it } from "vitest";
import { orderTimeline } from "../lib/timeline";
import { useTranscript } from "./transcript";

const seg = (id: string, utt: string, s: number, e: number, source: "app" | "mic" = "app") => ({
  id,
  utteranceId: utt,
  source,
  tStartMs: s,
  tEndMs: e,
  text: id,
  edited: false,
});

describe("transcript store", () => {
  beforeEach(() => useTranscript.getState().clear());

  it("replaces a partial when the final line with the same utterance arrives", () => {
    const st = useTranscript.getState();
    st.setPartial({ source: "app", utteranceId: "utt_000001", text: "こんにち" });
    expect(useTranscript.getState().partials.app?.text).toBe("こんにち");
    st.addSegment(seg("seg_000001", "utt_000001", 0, 1000));
    expect(useTranscript.getState().partials.app).toBeUndefined();
  });

  it("keeps a partial for a different utterance", () => {
    const st = useTranscript.getState();
    st.setPartial({ source: "app", utteranceId: "utt_000002", text: "次の" });
    st.addSegment(seg("seg_000001", "utt_000001", 0, 1000));
    expect(useTranscript.getState().partials.app?.utteranceId).toBe("utt_000002");
  });

  it("ignores a late partial for an utterance that is already final", () => {
    const st = useTranscript.getState();
    st.addSegment(seg("seg_000001", "utt_000001", 0, 1000));
    st.setPartial({ source: "app", utteranceId: "utt_000001", text: "古い" });
    expect(useTranscript.getState().partials.app).toBeUndefined();
  });

  it("an empty partial clears the provisional text", () => {
    const st = useTranscript.getState();
    st.setPartial({ source: "mic", utteranceId: "utt_000003", text: "えー" });
    st.setPartial({ source: "mic", utteranceId: "utt_000003", text: "" });
    expect(useTranscript.getState().partials.mic).toBeUndefined();
  });

  it("inserts late segments in timeline order, not append order", () => {
    const st = useTranscript.getState();
    st.addScreenshot({ id: "img_0001", tMs: 3000, thumbDataUrl: "data:x", width: 10, height: 10, file: "images/0001.png" });
    st.addSegment(seg("seg_000002", "utt_000002", 5000, 6000, "mic"));
    st.addSegment(seg("seg_000001", "utt_000001", 1000, 4000));
    const ids = orderTimeline(useTranscript.getState().items).map((i) => i.id);
    expect(ids).toEqual(["seg_000001", "img_0001", "seg_000002"]);
    expect(useTranscript.getState().thumbs.img_0001).toBe("data:x");
  });

  it("removes items and ignores duplicates", () => {
    const st = useTranscript.getState();
    st.addSegment(seg("seg_000001", "utt_000001", 0, 1000));
    st.addSegment(seg("seg_000001", "utt_000001", 0, 1000));
    expect(useTranscript.getState().items).toHaveLength(1);
    st.removeItem("seg_000001");
    expect(useTranscript.getState().items).toHaveLength(0);
  });
});

import { FileCode, FileText, FileType } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../ipc/commands";
import type { SessionSummary } from "../ipc/types";
import { exportAs, reportError, type ExportKind } from "../lib/actions";
import { dateTimeOf, hms } from "../lib/format";
import { useRecording } from "../store/recording";
import { useUi } from "../store/ui";
import { Button, Dialog, Spinner, Switch } from "./ui";

/** Flow D: offer to recover a session that has no end marker. */
export function RecoveryDialog() {
  const { t } = useTranslation();
  const [pending, setPending] = useState<SessionSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const state = useRecording((s) => s.state);

  useEffect(() => {
    if (state !== "ready") return;
    commands.listRecoverable().then(setPending).catch(() => {});
    // Only once per launch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const current = pending[0];
  if (!current) return null;
  const recover = async () => {
    setBusy(true);
    try {
      await commands.recoverSession(current.id);
      useUi.getState().toast(t("recovered"));
    } catch (e) {
      reportError(e, "saveFailed");
    } finally {
      setBusy(false);
      setPending((p) => p.slice(1));
    }
  };
  return (
    <Dialog
      title={t("recoverTitle")}
      onClose={() => setPending((p) => p.slice(1))}
      actions={
        <>
          <Button onClick={() => setPending((p) => p.slice(1))}>{t("recoverLater")}</Button>
          <Button variant="primary" onClick={recover} disabled={busy}>
            {t("recover")}
          </Button>
        </>
      }
    >
      <div className="rounded-xl bg-surface-2 px-3 py-2.5">
        <div className="font-semibold">{current.title}</div>
        <div className="mt-0.5 text-xs text-muted">
          {dateTimeOf(current.startedAt)} · {hms(current.durationMs)} · {t("segmentsCount", { n: current.segments })}
        </div>
      </div>
    </Dialog>
  );
}

/** Quitting while recording asks first (FR-93). */
export function ConfirmQuitDialog() {
  const { t } = useTranslation();
  const { confirmQuit, setConfirmQuit } = useUi();
  if (!confirmQuit) return null;
  return (
    <Dialog
      title={t("confirmQuitTitle")}
      onClose={() => setConfirmQuit(false)}
      actions={
        <>
          <Button onClick={() => setConfirmQuit(false)}>{t("cancel")}</Button>
          <Button
            variant="danger"
            onClick={() => {
              setConfirmQuit(false);
              void commands.quitApp();
            }}
          >
            {t("quitAndSave")}
          </Button>
        </>
      }
    >
      {t("confirmQuitBody")}
    </Dialog>
  );
}

/** 書き出し's formats; each asks where to save it. Markdown can come as a folder or as one ZIP. */
const FORMATS: { kind: ExportKind; icon: typeof FileText; title: string }[] = [
  { kind: "markdown", icon: FileText, title: "formatMarkdown" },
  { kind: "pdf", icon: FileType, title: "formatPdf" },
  { kind: "typst", icon: FileCode, title: "formatTypst" },
];

const ZIP_KEY = "kk-export-zip";

/** 書き出し: pick a format, then where to save it. Markdown's ZIP switch is remembered. */
export function ExportDialog({ sessionId, onClose }: { sessionId: string; onClose: () => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<ExportKind | null>(null);
  const [zip, setZip] = useState(() => {
    try {
      return localStorage.getItem(ZIP_KEY) === "1";
    } catch {
      return false;
    }
  });
  const toggleZip = (on: boolean) => {
    setZip(on);
    try {
      localStorage.setItem(ZIP_KEY, on ? "1" : "0");
    } catch {
      // Not remembered; the switch still works.
    }
  };
  const run = async (kind: ExportKind) => {
    setBusy(kind);
    await exportAs(kind, sessionId, zip);
    onClose();
  };
  return (
    <Dialog
      title={t("export")}
      onClose={busy ? undefined : onClose}
      actions={
        <Button onClick={onClose} disabled={!!busy}>
          {t("cancel")}
        </Button>
      }
    >
      <div className="space-y-1.5">
        {FORMATS.map(({ kind, icon: Icon, title }) => (
          <div key={kind} className="flex items-center rounded-xl border border-line bg-surface transition-colors hover:border-line-strong hover:bg-surface-2">
            <button
              type="button"
              disabled={!!busy}
              onClick={() => void run(kind)}
              className="flex min-w-0 flex-1 items-center gap-3 rounded-xl px-3 py-2.5 text-left disabled:pointer-events-none"
            >
              <Icon size={18} className="shrink-0 text-muted" aria-hidden />
              <span className="min-w-0 flex-1 text-[13.5px] font-semibold">{t(title)}</span>
              {busy === kind && <Spinner />}
            </button>
            {kind === "markdown" && (
              <label className="flex shrink-0 cursor-pointer items-center gap-2 py-2.5 pr-3 pl-1 text-[11.5px] font-semibold text-muted">
                ZIP
                <Switch checked={zip} onChange={toggleZip} disabled={!!busy} label={t("zipToggle")} />
              </label>
            )}
          </div>
        ))}
      </div>
    </Dialog>
  );
}

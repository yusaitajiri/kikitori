import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmQuitDialog, ExportDialog, RecoveryDialog } from "./components/Dialogs";
import { Transcript, TranscriptTail } from "./components/LiveTranscript";
import { Banners, Toasts } from "./components/Notices";
import { Deck } from "./components/Deck";
import { LiveHeader } from "./components/LiveHeader";
import { NewProjectDialog } from "./components/Projects";
import { SessionHeader } from "./components/SessionHeader";
import { StartScreen } from "./components/StartScreen";
import { TitleBar } from "./components/TitleBar";
import { Spinner } from "./components/ui";
import i18n from "./i18n";
import { useLiveSource } from "./hooks/useLiveSource";
import { commands } from "./ipc/commands";
import { on } from "./ipc/events";
import type { StatePayload, UiState } from "./ipc/types";
import { showNotice } from "./lib/actions";
import { hearAppLevels } from "./store/appLevels";
import { lineHistory } from "./store/line";
import { isBusy, useRecording } from "./store/recording";
import { useSettings } from "./store/settings";
import { useSource } from "./store/source";
import { useTranscript } from "./store/transcript";
import { useUi } from "./store/ui";
import { History } from "./views/History";
import { ModelManager } from "./views/ModelManager";
import { SessionViewer } from "./views/SessionViewer";
import { Settings } from "./views/Settings";
import { SetupWizard } from "./views/SetupWizard";

type Phase = "start" | "live" | "review";

/** What the main view shows: nothing yet, a recording, or the session that just finished. */
const phaseOf = (state: UiState, sessionId: string | undefined): Phase => (isBusy(state) ? "live" : sessionId ? "review" : "start");

/** Loads the transcript of a session the backend reports as active. */
async function followSession(sessionId: string | undefined) {
  if (!sessionId) return;
  const tr = useTranscript.getState();
  if (tr.sessionId === sessionId) return;
  tr.clear();
  useTranscript.setState({ sessionId });
  try {
    tr.load(await commands.getSession(sessionId));
  } catch {
    // The live events still fill the view.
  }
}

function isLive(): boolean {
  const live = useRecording.getState().sessionId;
  return !!live && useTranscript.getState().sessionId === live;
}

function applyState(p: StatePayload) {
  const before = useRecording.getState().state;
  // The line keeps its own history: a new recording starts an empty one, a resume leaves a gap.
  if (p.state === "recording" && p.sessionId) {
    // The same session again after a pause, or continued after it was saved (FR-08): a gap.
    const again = lineHistory.sessionId === p.sessionId;
    lineHistory.start(p.sessionId);
    if (again && before !== "recording") lineHistory.gap();
  }
  useRecording.getState().applyState(p);
  void followSession(p.sessionId);
}

function useBackendEvents() {
  useEffect(() => {
    const rec = useRecording.getState();
    const tr = useTranscript.getState;
    const subs = [
      on("recording://state", applyState),
      on("audio://levels", (p) => {
        if (useRecording.getState().state === "recording") lineHistory.push(p);
      }),
      on("audio://app-levels", hearAppLevels),
      // Live events belong to the live session.
      on("transcript://segment", (p) => isLive() && tr().addSegment(p)),
      on("transcript://partial", (p) => isLive() && tr().setPartial(p)),
      on("transcript://segment-removed", (p) => isLive() && tr().removeItem(p.id)),
      on("transcript://segment-updated", (p) => isLive() && tr().updateSegment(p)),
      on("transcript://screenshot", (p) => {
        if (!isLive()) return;
        tr().addScreenshot(p);
        lineHistory.shot();
      }),
      on("transcript://marker", (p) => isLive() && tr().addMarker(p)),
      on("asr://lag", (p) => rec.setLag(p)),
      on("finishing://progress", (p) => rec.setFinishing(p)),
      on("model://status", (p) => {
        rec.setEngine(p);
        void useSettings.getState().refreshInfo();
        void useSettings.getState().refreshModels().catch(() => {});
      }),
      on("session://saved", (p) => {
        rec.setSaved(p.folder);
        useUi.getState().toast(i18n.t("saved"));
        void commands.getSession(p.sessionId).then((s) => {
          if (useTranscript.getState().sessionId === p.sessionId) useTranscript.getState().load(s);
        });
      }),
      on("app://notice", (n) => showNotice(n)),
      on("ui://command", (c) => {
        if (c.command === "confirm_quit") useUi.getState().setConfirmQuit(true);
      }),
    ];
    return () => {
      for (const s of subs) void s.then((un) => un());
    };
  }, []);
}

function MainView({ compact }: { compact: boolean }) {
  const state = useRecording((s) => s.state);
  const sessionId = useTranscript((s) => s.sessionId);
  const folder = useTranscript((s) => s.folder);
  const phase = phaseOf(state, sessionId);

  // The deck stays mounted from the start dot to the finished session, so its line can move
  // between them; only what is above it changes.
  let body;
  if (phase === "start") {
    body = <StartScreen compact={compact} />;
  } else if (compact) {
    body = (
      <div className="flex min-h-0 flex-1 flex-col justify-end overflow-hidden px-3 pt-0.5 pb-1">
        <TranscriptTail />
      </div>
    );
  } else if (phase === "live") {
    // The recording keeps its title and source in view above the transcript.
    body = (
      <>
        {state !== "finishing" && <LiveHeader />}
        <Transcript live editable={false} />
      </>
    );
  } else {
    body = <Transcript live={false} editable header={<SessionHeader />} />;
  }

  return (
    <>
      <Banners />
      {/* Before a recording the expanded window gives the free space to the deck, whose dot sits in its middle. */}
      <div key={phase === "start" ? "start" : "session"} className={`flex min-h-0 animate-fade-in flex-col ${phase === "start" && !compact ? "shrink-0" : "flex-1"}`}>
        {body}
      </div>
      <Deck compact={compact} phase={phase} sessionId={sessionId} folder={folder} onNew />
    </>
  );
}

export default function App() {
  const { t } = useTranslation();
  const { settings, info, load } = useSettings();
  const view = useUi((s) => s.view);
  const flash = useUi((s) => s.flash);
  const exportFor = useUi((s) => s.exportFor);
  const setExportFor = useUi((s) => s.setExportFor);
  const state = useRecording((s) => s.state);

  useBackendEvents();
  useLiveSource(state === "recording" || state === "paused", useRecording((s) => s.sessionId));

  useEffect(() => {
    void (async () => {
      await load();
      const s = useSettings.getState();
      if (s.settings) useSource.getState().initFrom(s.settings);
      if (s.info) {
        useRecording.getState().applyState(s.info.state);
        useRecording.getState().setEngine(s.info.engine);
        void followSession(s.info.state.sessionId);
      }
      void useSettings.getState().refreshModels().catch(() => {});
      void useSource.getState().refreshDevices().catch(() => {});
    })();
  }, [load]);

  const needsWizard = !!info && (!info.setupDone || state === "needs_model");
  const layout = settings?.window.layout ?? "compact";
  const effectiveLayout = needsWizard || view !== "main" ? "expanded" : layout;

  useEffect(() => {
    if (!settings) return;
    void commands.setWindowLayout(effectiveLayout, false).catch(() => {});
  }, [effectiveLayout, settings]);

  if (!settings || !info) {
    return (
      <div className="flex h-full items-center justify-center gap-2 text-muted">
        <Spinner />
        {t("loading")}
      </div>
    );
  }

  let body;
  if (needsWizard) {
    body = <SetupWizard />;
  } else if (view === "settings") {
    body = <Settings />;
  } else if (view === "history") {
    body = <History />;
  } else if (view === "models") {
    body = <ModelManager standalone />;
  } else if (view === "session") {
    body = <SessionViewer />;
  } else {
    body = <MainView compact={layout === "compact"} />;
  }

  return (
    <div className="kk-shell relative flex h-full flex-col overflow-hidden border border-line bg-bg">
      {/* One title bar for every view, outside the keyed view, so it stays put while they change. */}
      <TitleBar compact={effectiveLayout === "compact"} setup={needsWizard} />
      <div key={needsWizard ? "wizard" : view} className="flex min-h-0 flex-1 flex-col">
        {body}
      </div>
      {flash > 0 && <div key={flash} className="kk-flash pointer-events-none fixed inset-0 z-50" aria-hidden />}
      <Toasts />
      <RecoveryDialog />
      <ConfirmQuitDialog />
      {exportFor && <ExportDialog sessionId={exportFor} onClose={() => setExportFor(undefined)} />}
      <NewProjectDialog />
    </div>
  );
}

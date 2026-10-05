import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Transcript } from "../components/LiveTranscript";
import { Deck } from "../components/Deck";
import { SessionHeader } from "../components/SessionHeader";
import { Spinner } from "../components/ui";
import { commands } from "../ipc/commands";
import { reportError } from "../lib/actions";
import { createTranscriptStore, TranscriptContext } from "../store/transcript";
import { useUi } from "../store/ui";

/**
 * A past session: read, edit lines, rename, copy, export (FR-41, FR-62). It has its own
 * transcript store, so a recording that runs meanwhile keeps its own.
 */
export function SessionViewer() {
  const { t } = useTranslation();
  const viewingSessionId = useUi((s) => s.viewingSessionId);
  const store = useMemo(() => createTranscriptStore(), []);
  const sessionId = store((s) => s.sessionId);
  const folder = store((s) => s.folder);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!viewingSessionId) return;
    let alive = true;
    commands
      .getSession(viewingSessionId)
      .then((s) => alive && store.getState().load(s))
      .catch((e) => {
        reportError(e);
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, [viewingSessionId, store]);

  return (
    <TranscriptContext.Provider value={store}>
      <div className="flex h-full flex-col">
        {sessionId ? (
          <>
            <Transcript editable header={<SessionHeader />} />
            <Deck phase="review" sessionId={sessionId} folder={folder} />
          </>
        ) : (
          <div className="flex flex-1 items-center justify-center text-muted">{failed ? t("loadFailed") : <Spinner size={20} />}</div>
        )}
      </div>
    </TranscriptContext.Provider>
  );
}

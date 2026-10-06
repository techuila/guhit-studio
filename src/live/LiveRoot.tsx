// Mounted once by App.tsx, for the hub and the editor alike: starts the live
// session wiring and shows the live session's dialogs, the question before
// taking back someone else's step, and the one before leaving a hosted
// session.
import { useEffect } from "react";
import { leaveEditor } from "../shell/actions";
import { useApp } from "../state/store";
import { ConfirmDialog } from "../ui/Dialog";
import { Presence, useLastTruthy } from "../ui/motionDom";
import { others } from "./format";
import { JoinDialog } from "./JoinDialog";
import { useLive } from "./liveStore";
import { ShareDialog } from "./ShareDialog";
import { startLive } from "./wiring";

export function LiveRoot() {
  // After boot, never at module load: the dev harness installs first.
  useEffect(() => startLive(), []);
  const dialog = useLive((st) => st.dialog);
  const lastDialog = useLastTruthy(dialog);
  return (
    <>
      <Presence open={dialog !== null} exit="panel">
        {(stage) => (lastDialog === "join" ? <JoinDialog stage={stage} /> : lastDialog === "share" ? <ShareDialog stage={stage} /> : null)}
      </Presence>
      <UndoConfirm />
      <LeaveConfirm />
    </>
  );
}

/** An undo or redo of someone else's step asks first (`useApp().undoConfirm`). */
function UndoConfirm() {
  const pending = useApp((st) => st.undoConfirm);
  const last = useLastTruthy(pending);
  const resolve = useApp((st) => st.resolveUndoConfirm);
  return (
    <Presence open={pending !== null} exit="panel">
      {(stage) =>
        last ? (
          <ConfirmDialog
            title={last.redo ? "Redo someone else's change?" : "Undo someone else's change?"}
            message={last.message}
            confirmLabel={last.redo ? "Redo anyway" : "Undo anyway"}
            onConfirm={() => void resolve(true)}
            onCancel={() => void resolve(false)}
            stage={stage}
          />
        ) : null
      }
    </Presence>
  );
}

/** Going back to the projects while hosting ends the session for everyone: ask. */
function LeaveConfirm() {
  const open = useLive((st) => st.askLeave);
  const count = useLive((st) => others(st.status).length);
  const close = () => useLive.getState().setAskLeave(false);
  return (
    <Presence open={open} exit="panel">
      {(stage) => (
        <ConfirmDialog
          title="End the live session?"
          message={
            count > 0
              ? `Going back to your projects ends the live session for ${count === 1 ? "the other person" : `the other ${count} people`}. The project stays on this computer.`
              : "Going back to your projects ends the live session. The project stays on this computer."
          }
          confirmLabel="End and go back"
          danger
          onCancel={close}
          onConfirm={() => {
            close();
            void leaveEditor(true);
          }}
          stage={stage}
        />
      )}
    </Presence>
  );
}

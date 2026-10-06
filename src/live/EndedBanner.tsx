// On the hub after a live session you joined ended (the host ended it, or
// the connection was lost): its last copy can still be saved as your own
// project, until another project opens.
import { useState } from "react";
import { Button, IconButton } from "../ui/controls";
import { Icon } from "../ui/icons";
import { usePresence } from "../ui/motion";
import { useLastTruthy } from "../ui/motionDom";
import { useLive } from "./liveStore";
import { saveCopy } from "./session";
import s from "./live.module.css";

export function EndedBanner({ onSaved }: { onSaved: () => void }) {
  const ended = useLive((st) => st.ended);
  const last = useLastTruthy(ended);
  const p = usePresence(ended !== null, "base");
  const [busy, setBusy] = useState(false);
  if (!p.mounted || !last) return null;
  const save = async () => {
    setBusy(true);
    const ok = await saveCopy();
    setBusy(false);
    if (!ok) return;
    useLive.getState().setEnded(null);
    onSaved();
  };
  return (
    <div className={s.ended} data-stage={p.stage} role="status">
      <Icon name="people" size={16} />
      <span className={s.endedText}>
        The live session{last.projectName ? <> on <strong>{last.projectName}</strong></> : null} ended. Save a copy to keep working on it here.
      </span>
      <Button size="sm" icon="folder" disabled={busy} onClick={() => void save()}>
        {busy ? "Saving" : "Save a copy"}
      </Button>
      <IconButton icon="close" label="Dismiss" tip="Dismiss" tipSide="bottom-end" onClick={() => useLive.getState().setEnded(null)} />
    </div>
  );
}

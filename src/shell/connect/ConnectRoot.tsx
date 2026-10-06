// Mounted once by App.tsx, for the hub and the editor alike.
import { Presence } from "../../ui/motionDom";
import { ConnectDialog } from "./ConnectDialog";
import { useConnect } from "./connectStore";

export function ConnectRoot() {
  const open = useConnect((st) => st.open);
  return <Presence open={open} exit="panel">{(stage) => <ConnectDialog stage={stage} />}</Presence>;
}

import { useCallback, useEffect, useState } from "react";
import type { IpcError, SnapshotMeta } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { useApp } from "../state/store";
import { ConfirmDialog, Dialog } from "../ui/Dialog";
import { Button, Spinner, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { Presence, useLastTruthy } from "../ui/motionDom";
import { relativeTime } from "../ui/units";
import s from "./overlays.module.css";

function exactTime(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  return new Date(t).toLocaleString("en-PH", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

export function VersionsPanel({ onClose, stage }: { onClose: () => void; stage?: PresenceStage }) {
  const setDoc = useApp((st) => st.setDoc);
  const toast = useApp((st) => st.toast);
  const reportError = useApp((st) => st.reportError);
  const revision = useApp((st) => st.doc?.revision ?? 0);

  const [list, setList] = useState<SnapshotMeta[] | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [label, setLabel] = useState("");
  const [saving, setSaving] = useState(false);
  const [restoring, setRestoring] = useState<SnapshotMeta | null>(null);
  const lastRestoring = useLastTruthy(restoring);

  const refresh = useCallback(async () => {
    try {
      const items = await ipc.snapshotList();
      setList([...items].sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at)));
      setError(null);
    } catch (e) {
      setError(toIpcError(e));
      setList((l) => l ?? []);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const create = async () => {
    const name = label.trim() || `Version at revision ${revision}`;
    setSaving(true);
    try {
      const meta = await ipc.snapshotCreate(name);
      setLabel("");
      toast("success", `Saved version "${meta.label}"`);
      await refresh();
    } catch (e) {
      reportError(e);
    } finally {
      setSaving(false);
    }
  };

  const restore = async (snap: SnapshotMeta) => {
    setRestoring(null);
    try {
      setDoc(await ipc.snapshotRestore(snap.id));
      toast("success", `Restored "${snap.label}". The state before it was kept as an automatic version.`);
      onClose();
    } catch (e) {
      reportError(e);
    }
  };

  return (
    <>
      <Dialog title="Versions" onClose={onClose} placement="side" width={360} stage={stage}>
        <form
          className={s.versionNew}
          onSubmit={(e) => {
            e.preventDefault();
            if (!saving) void create();
          }}
        >
          <label htmlFor="version-label">Save the plan as it is now</label>
          <div className={s.versionNewRow}>
            <input
              id="version-label"
              data-autofocus
              type="text"
              value={label}
              placeholder="For example: Option A"
              spellCheck={false}
              onChange={(e) => setLabel(e.target.value)}
            />
            <Button variant="primary" disabled={saving} onClick={() => void create()}>
              Save version
            </Button>
          </div>
          <p>Every change is already saved. A version is a point you can come back to.</p>
        </form>

        {list === null ? (
          <div className={s.versionState}>
            <Spinner />
          </div>
        ) : error ? (
          <div className={s.versionState}>
            <Icon name="warning" size={20} />
            <span>{error.message}</span>
            <Button size="sm" onClick={() => void refresh()}>
              Try again
            </Button>
          </div>
        ) : list.length === 0 ? (
          <div className={s.versionState}>
            <Icon name="versions" size={22} />
            <span>No versions yet. Save one before a big change.</span>
          </div>
        ) : (
          <ul className={s.versionList}>
            {list.map((snap) => (
              <li key={snap.id} className={cx(s.version, snap.auto && s.versionAuto)}>
                <span className={s.versionDot} aria-hidden />
                <div className={s.versionText}>
                  <strong>{snap.label}</strong>
                  <span title={exactTime(snap.created_at)}>
                    {relativeTime(snap.created_at)}
                    {snap.auto ? ", automatic" : ""}
                  </span>
                </div>
                <Button size="sm" onClick={() => setRestoring(snap)}>
                  Restore
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Dialog>

      <Presence open={restoring !== null} exit="panel">
        {(confirmStage) => {
          const shown = lastRestoring;
          return shown ? (
            <ConfirmDialog
              title="Restore this version?"
              message={
                <>
                  The plan goes back to <strong>{shown.label}</strong> from {relativeTime(shown.created_at)}. Your current
                  plan is kept as an automatic version first, so you can return to it. Undo history starts over.
                </>
              }
              confirmLabel="Restore version"
              onConfirm={() => void restore(shown)}
              onCancel={() => setRestoring(null)}
              stage={confirmStage}
            />
          ) : null;
        }}
      </Presence>
    </>
  );
}

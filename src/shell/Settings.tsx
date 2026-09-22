// App settings. One section today: DWG interchange through the ODA File
// Converter (DECISIONS D16). More sections land here as the app grows.
import { useEffect, useState } from "react";
import type { DwgConverterStatus } from "../contract/bindings";
import { ipc, isTauri } from "../contract/ipc";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, Field, Section, Spinner, TextField, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { useSection } from "./shellStore";
import s from "./overlays.module.css";

/** Official Open Design Alliance download page for the free ODA File Converter. */
const ODA_DOWNLOAD_URL = "https://www.opendesign.com/guestfiles/oda_file_converter";

export function SettingsDialog({ onClose, stage }: { onClose: () => void; stage?: PresenceStage }) {
  const reportError = useApp((st) => st.reportError);
  const toast = useApp((st) => st.toast);
  const [open, toggle] = useSection("settings-interchange", true);
  const [status, setStatus] = useState<DwgConverterStatus | null>(null);
  const [path, setPath] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void ipc
      .dwgStatus()
      .then((st) => {
        if (cancelled) return;
        setStatus(st);
        setPath(st.path ?? "");
        setLoading(false);
      })
      .catch((e) => {
        if (cancelled) return;
        reportError(e);
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [reportError]);

  const save = async (next: string) => {
    setSaving(true);
    try {
      const st = await ipc.dwgSetPath(next);
      setStatus(st);
      setPath(st.path ?? "");
      toast("success", next === "" ? "ODA File Converter path cleared" : st.works ? "ODA File Converter found" : st.message);
    } catch (e) {
      reportError(e);
    } finally {
      setSaving(false);
    }
  };

  const browse = async () => {
    if (!isTauri) return;
    const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
    const picked = await openDialog({ multiple: false, title: "Locate the ODA File Converter" });
    if (typeof picked === "string") void save(picked);
  };

  return (
    <Dialog title="Settings" onClose={onClose} width={520} stage={stage}>
      <Section title="Interchange" icon="import" open={open} onToggle={toggle}>
        <div className={s.settingsSection}>
          <p className={s.exportNote}>
            DWG import and export go through the free ODA File Converter (DECISIONS D16), which this app detects on your
            computer. DXF, IFC, glTF, OBJ and Collada work without it.
          </p>
          <Field label="Converter" hint="Path to the ODA File Converter">
            <div className={s.settingsPathRow}>
              <TextField label="ODA File Converter path" className={s.settingsPathInput} value={path} placeholder="Not set" onCommit={(v) => void save(v)} />
              {isTauri ? (
                <Button size="sm" disabled={saving} onClick={() => void browse()}>
                  Browse
                </Button>
              ) : null}
            </div>
          </Field>
          {loading ? (
            <Spinner size={16} />
          ) : status ? (
            <p className={cx(s.settingsStatus, !status.works && s.settingsStatusBad)}>
              <Icon name={status.works ? "check" : "warning"} size={14} />
              {status.message}
            </p>
          ) : null}
          <p className={s.exportNote}>
            Do not have it yet?{" "}
            <a href={ODA_DOWNLOAD_URL} target="_blank" rel="noreferrer">
              Download the ODA File Converter
            </a>{" "}
            (free, from the Open Design Alliance), install it, then point this at it.
          </p>
        </div>
      </Section>
    </Dialog>
  );
}

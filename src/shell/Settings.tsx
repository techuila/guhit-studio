// App settings. Two sections: DWG interchange through the ODA File Converter
// (DECISIONS D16) and AI rendering through Google's image models (D17).
// More sections land here as the app grows.
import { useEffect, useRef, useState } from "react";
import type { DwgConverterStatus, RenderAiSettings } from "../contract/bindings";
import { ipc, isTauri, toIpcError } from "../contract/ipc";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, Field, Section, Select, Spinner, TextField, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { useRenderUi } from "../viewer3d/render/renderStore";
import { useSection } from "./shellStore";
import s from "./overlays.module.css";

/** Official Open Design Alliance download page for the free ODA File Converter. */
const ODA_DOWNLOAD_URL = "https://www.opendesign.com/guestfiles/oda_file_converter";

/** Where a Google AI Studio key comes from. */
const GOOGLE_KEY_URL = "https://aistudio.google.com/apikey";

const RENDER_MODELS = [
  { value: "gemini-3.1-flash-image", label: "Gemini 3.1 Flash Image (default)" },
  { value: "gemini-3-pro-image", label: "Gemini 3 Pro Image (high)" },
];

/**
 * AI rendering: the Google AI Studio key (write only, it never comes back),
 * the model, and what the images are. The studio's "Add Google AI key" button
 * opens this dialog and this section reveals itself.
 */
function AiRenderingSection() {
  const reportError = useApp((st) => st.reportError);
  const toast = useApp((st) => st.toast);
  const [open, toggle] = useSection("settings-ai-render", true);
  const focusToken = useRenderUi((st) => st.settingsFocus);
  const [settings, setSettings] = useState<RenderAiSettings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    void ipc
      .renderAiSettingsGet()
      .then((st) => {
        if (cancelled) return;
        setSettings(st);
        setLoadError(null);
      })
      .catch((e) => {
        if (cancelled) return;
        const err = toIpcError(e);
        setLoadError(
          err.code === "unknown_command"
            ? "This build's backend has no AI rendering yet."
            : err.message,
        );
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Opened from the studio: show this section and bring it into view.
  useEffect(() => {
    if (focusToken === 0) return;
    if (!open) toggle();
    requestAnimationFrame(() => bodyRef.current?.scrollIntoView({ block: "nearest" }));
    // Only react to a new request, not to every toggle.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusToken]);

  const save = async (apiKey: string | null, model: string | null, done: string) => {
    setSaving(true);
    try {
      const next = await ipc.renderAiSettingsSet(apiKey, model);
      setSettings(next);
      setKey("");
      toast("success", done);
    } catch (e) {
      reportError(e);
    } finally {
      setSaving(false);
    }
  };

  const hasKey = settings?.has_api_key ?? false;

  return (
    <Section title="AI rendering" icon="visuals" open={open} onToggle={toggle}>
      <div className={s.settingsSection} ref={bodyRef} data-testid="settings-ai-render">
        <p className={s.exportNote}>
          AI visualizations come from Google's Gemini image models with your own Google AI Studio key (DECISIONS D17).
          The key is kept in the app's data folder, readable only by your user account, and it is never shown again
          here or written into a project.
        </p>
        {loadError ? (
          <p className={cx(s.settingsStatus, s.settingsStatusBad)}>
            <Icon name="warning" size={14} />
            {loadError}
          </p>
        ) : settings === null ? (
          <Spinner size={16} />
        ) : (
          <>
            <Field label="Google AI key" hint="From aistudio.google.com">
              <div className={s.settingsPathRow}>
                <TextField
                  label="Google AI Studio API key"
                  className={s.settingsPathInput}
                  value={key}
                  placeholder={hasKey ? "Saved. Paste a new key to replace it" : "Paste your key"}
                  onCommit={(v) => {
                    setKey("");
                    if (v.trim()) void save(v.trim(), null, "Google AI key saved");
                  }}
                />
                {hasKey ? (
                  <Button size="sm" disabled={saving} onClick={() => void save("", null, "Google AI key removed")}>
                    Remove
                  </Button>
                ) : null}
              </div>
            </Field>
            <Field label="Model" hint="Flash is the everyday model, Pro is the high setting">
              <Select
                label="Image model"
                value={settings.model || RENDER_MODELS[0].value}
                options={
                  !settings.model || RENDER_MODELS.some((m) => m.value === settings.model)
                    ? RENDER_MODELS
                    : [...RENDER_MODELS, { value: settings.model, label: settings.model }]
                }
                onChange={(model) => void save(null, model, "Image model saved")}
              />
            </Field>
            <p className={cx(s.settingsStatus, !hasKey && s.settingsStatusBad)}>
              <Icon name={hasKey ? "check" : "warning"} size={14} />
              {hasKey ? `A key is configured. Cost: ${settings.cost_hint}` : "No key yet. Rendering stays off until you add one."}
            </p>
          </>
        )}
        <p className={s.exportNote}>
          Every image carries an invisible SynthID watermark from Google and is labelled "AI visualization" in the app.
          The images are for discussion, not construction documents, and they never change the model.{" "}
          <a href={GOOGLE_KEY_URL} target="_blank" rel="noreferrer">
            Get a Google AI Studio key
          </a>
          .
        </p>
      </div>
    </Section>
  );
}

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
      <AiRenderingSection />
    </Dialog>
  );
}

// Copilot settings: API key (write only) and model name. The key goes to the
// backend's private key file and is never read back into the UI.
import { useEffect, useRef, useState } from "react";
import type { AiSettings } from "../contract/bindings";
import { ipc, toIpcError } from "../contract/ipc";
import { CloseIcon } from "./icons";
import s from "./AiDock.module.css";

interface Props {
  settings: AiSettings | null;
  loadError: string | null;
  /** From `usePresence` in the dock: the popover grows in and animates out. */
  stage?: "enter" | "idle" | "exit";
  onClose: () => void;
  onSaved: (settings: AiSettings) => void;
}

export function SettingsPopover({ settings, loadError, stage = "idle", onClose, onSaved }: Props) {
  const [key, setKey] = useState("");
  const [model, setModel] = useState(settings?.model ?? "");
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error" | "info"; text: string } | null>(null);
  const keyRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setModel(settings?.model ?? "");
  }, [settings?.model]);

  useEffect(() => {
    keyRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const run = async (apiKey: string | null, nextModel: string | null, done: (next: AiSettings) => string, tone: "ok" | "info" = "ok") => {
    setSaving(true);
    setMessage(null);
    try {
      const next = await ipc.aiSettingsSet(apiKey, nextModel);
      onSaved(next);
      // The typed key is dropped from component state as soon as it is sent.
      setKey("");
      setMessage({ tone: apiKey === "" && next.has_api_key ? "info" : tone, text: done(next) });
    } catch (e) {
      setMessage({ tone: "error", text: toIpcError(e).message });
    } finally {
      setSaving(false);
    }
  };

  const save = () => {
    const trimmedKey = key.trim();
    const trimmedModel = model.trim();
    const modelChanged = trimmedModel !== (settings?.model ?? "");
    if (!trimmedKey && !modelChanged) {
      setMessage({ tone: "info", text: "Nothing to save." });
      return;
    }
    void run(trimmedKey || null, modelChanged ? trimmedModel : null, () => (trimmedKey ? "Key saved." : "Model saved."));
  };

  const remove = () =>
    void run("", null, (next) =>
      next.has_api_key
        ? "Key removed. A key is still provided by the ANTHROPIC_API_KEY environment variable."
        : "Key removed.",
    );

  const hasKey = settings?.has_api_key ?? false;

  return (
    <div className={s.popover} data-stage={stage} data-testid="ai-settings" role="dialog" aria-label="Copilot settings">
      <header className={s.popHead}>
        <span className={s.popTitle}>Copilot settings</span>
        <button type="button" className={s.iconBtn} aria-label="Close settings" onClick={onClose}>
          <CloseIcon />
        </button>
      </header>

      <div className={s.keyStatus} data-on={hasKey}>
        <span className={s.keyDot} aria-hidden />
        {settings === null ? (loadError ?? "Checking") : hasKey ? "An API key is configured" : "No API key yet"}
      </div>

      <label className={s.field}>
        <span className={s.fieldLabel}>{hasKey ? "Replace API key" : "Claude API key"}</span>
        <input
          ref={keyRef}
          className={s.textInput}
          type="password"
          value={key}
          placeholder={hasKey ? "Saved. Paste a new key to replace it" : "sk-ant-..."}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
      </label>

      <label className={s.field}>
        <span className={s.fieldLabel}>Model</span>
        <input
          className={s.textInput}
          type="text"
          value={model}
          placeholder="claude-opus-5"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setModel(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
      </label>

      <p className={s.popText}>
        Use a Claude API key from platform.claude.com (it starts with sk-ant-api). Claude subscription and Claude
        Code tokens do not work. The key is kept in the app's data folder, readable only by your user account,
        never in a project file, and it cannot be shown again here.
      </p>

      {message ? (
        <p className={s.popMessage} data-tone={message.tone} role={message.tone === "error" ? "alert" : "status"}>
          {message.text}
        </p>
      ) : null}

      <div className={s.popActions}>
        <button type="button" className={s.primaryBtn} disabled={saving} onClick={save}>
          {saving ? "Saving" : "Save"}
        </button>
        {hasKey ? (
          <button type="button" className={s.discardBtn} disabled={saving} onClick={remove}>
            Remove key
          </button>
        ) : null}
      </div>
    </div>
  );
}

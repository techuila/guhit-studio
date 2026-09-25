// Join a live session: paste the invite, give your name, Join. The shared
// project opens in the editor once the host lets you in.
import { useEffect, useRef, useState } from "react";
import { useApp } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, Spinner, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import type { PresenceStage } from "../ui/motion";
import { INVITE_PREFIX, NAME_MAX, cleanName, parseInvite } from "./format";
import { useLive } from "./liveStore";
import { errorText, joinSession } from "./session";
import s from "./live.module.css";

export function JoinDialog({ stage }: { stage?: PresenceStage }) {
  const profileName = useLive((st) => st.profile?.name ?? "");
  const hasDoc = useApp((st) => st.doc !== null);
  const hosting = useLive((st) => st.status.mode === "hosting");
  const [invite, setInvite] = useState("");
  const [name, setName] = useState(profileName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const inviteRef = useRef<HTMLTextAreaElement>(null);

  // The profile can load after the dialog opens: fill it in until the user types.
  const touched = useRef(false);
  useEffect(() => {
    if (!touched.current) setName(profileName);
  }, [profileName]);

  const info = parseInvite(invite);
  const typed = invite.trim() !== "";
  const close = () => {
    if (!busy) useLive.getState().closeDialog();
  };

  const join = async () => {
    if (busy) return;
    if (!info) {
      setError(typed ? `This is not a Guhit invite. Guhit invites start with "${INVITE_PREFIX}".` : "Paste the invite the host sent you.");
      inviteRef.current?.focus();
      return;
    }
    if (cleanName(name) === "") {
      setError("Add your name first. Others see it next to your pointer.");
      nameRef.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await joinSession(invite, name);
      useLive.getState().closeDialog();
      useApp.getState().toast("success", info.project ? `Joined the live session on ${info.project}.` : "Joined the live session.");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title="Join a live session"
      onClose={close}
      width={460}
      stage={stage}
      footer={
        <>
          <Button onClick={close} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" icon={busy ? undefined : "join"} disabled={busy} onClick={() => void join()}>
            {busy ? (
              <span className={s.busyLabel}>
                <Spinner size={14} /> Joining
              </span>
            ) : (
              "Join"
            )}
          </Button>
        </>
      }
    >
      <form
        className={s.stack}
        onSubmit={(e) => {
          e.preventDefault();
          void join();
        }}
      >
        <label className={s.label} htmlFor="live-invite">
          Invite
        </label>
        <textarea
          id="live-invite"
          ref={inviteRef}
          className={cx(s.input, s.inviteInput, typed && !info && s.inputBad)}
          value={invite}
          rows={3}
          placeholder="Paste the invite the host sent you"
          spellCheck={false}
          autoComplete="off"
          disabled={busy}
          data-autofocus
          onChange={(e) => {
            setInvite(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void join();
            }
          }}
        />
        {info ? (
          <p key="ok" className={cx(s.hint, s.inviteOk)}>
            <Icon name="check" size={14} />
            <span>
              {info.project ? <strong>{info.project}</strong> : "A live session"}
              {info.addrs.length > 0 ? `, hosted at ${info.addrs[0]}` : ""}
            </span>
          </p>
        ) : (
          <p key="how" className={s.hint}>
            The host copies it from their Share button. You need to be on the same network or VPN.
          </p>
        )}

        <label className={s.label} htmlFor="live-join-name">
          Your name
        </label>
        <input
          id="live-join-name"
          ref={nameRef}
          className={s.input}
          value={name}
          maxLength={NAME_MAX}
          placeholder="How others see you"
          spellCheck={false}
          autoComplete="off"
          disabled={busy}
          onChange={(e) => {
            touched.current = true;
            setName(e.target.value);
            setError(null);
          }}
        />
        {hasDoc ? (
          <p className={s.hint}>{hosting ? "Joining ends the session you host and closes this project." : "Joining closes the project that is open now. It stays in your projects."}</p>
        ) : null}
        {error ? (
          <p key={error} className={s.error} role="alert">
            {error}
          </p>
        ) : null}
        <button type="submit" hidden />
      </form>
    </Dialog>
  );
}

"use client";

import { useState } from "react";
import { checkPassword, MIN_PASSWORD_LENGTH, strengthLabels } from "@/lib/vault";
import { Field, inputClass } from "./ui";

/**
 * State for choosing a new password: the two boxes, and whether they are acceptable. `onEdit` runs when
 * either box changes, so a form can clear an error that no longer applies.
 */
export function useNewPassword(onEdit?: () => void) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const edit = (set: (value: string) => void) => (value: string) => {
    set(value);
    onEdit?.();
  };
  const check = checkPassword(password);
  /** Why the password cannot be used yet, or null when it can. Show it only after an attempt. */
  const problem = !check.ok ? check.message : confirm !== password ? "The two passwords are different." : null;
  return { password, setPassword: edit(setPassword), confirm, setConfirm: edit(setConfirm), check, problem, reset: () => (setPassword(""), setConfirm("")) };
}

const bar = ["bg-neutral-300 dark:bg-neutral-700", "bg-red-500", "bg-amber-500", "bg-lime-500", "bg-emerald-500"];

/**
 * The two password boxes with a strength bar. Each box has an explicit accessible name, so forms on
 * the same page (create an account, sign in) can tell theirs apart.
 */
export function NewPasswordFields({
  state,
  names,
  label = "Password",
}: {
  state: ReturnType<typeof useNewPassword>;
  /** The accessible names of the two boxes. */
  names: { password: string; confirm: string };
  label?: string;
}) {
  const { password, setPassword, confirm, setConfirm, check } = state;
  return (
    <div className="space-y-3">
      <Field label={label} hint={`At least ${MIN_PASSWORD_LENGTH} characters. A few ordinary words together work well.`}>
        <input
          type="password"
          aria-label={names.password}
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputClass}
        />
      </Field>
      {password.length > 0 && (
        <div className="flex items-center gap-3" aria-live="polite">
          <div className="flex flex-1 gap-1" aria-hidden>
            {[1, 2, 3, 4].map((i) => (
              <span key={i} className={`h-1.5 flex-1 rounded-full ${i <= check.strength ? bar[check.strength] : bar[0]}`} />
            ))}
          </div>
          <span className="w-14 text-xs text-neutral-500">{strengthLabels[check.strength]}</span>
        </div>
      )}
      <Field label="Confirm password">
        <input
          type="password"
          aria-label={names.confirm}
          autoComplete="new-password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          className={inputClass}
        />
      </Field>
    </div>
  );
}

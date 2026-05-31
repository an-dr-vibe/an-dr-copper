import type { Api } from "@host/api";

// OS keychain service name — matches the original SafeInputKey Python app
// so existing keychain entries are reused if already set up there.
const SERVICE = "SafeInputKey";
const KEY_TEXT = "stored_text";

type Inputs = {
  action?: string;
  text?: string;
  [key: string]: unknown;
};

export default function (api: Api) {
  return {
    async onTrigger(inputs: Inputs = {}) {
      const action = String(inputs.action ?? "type-text");

      switch (action) {
        // ── type-text ────────────────────────────────────────────────────────
        // Reads the stored secret from the OS keychain and types it at the
        // current cursor position. This is the action mapped to a hotkey.
        case "type-text": {
          const text = (await api.secureStore.get(SERVICE, KEY_TEXT)) as
            | string
            | null;
          if (!text) {
            await api.notify(
              "Safe Input Key: no text saved — run setup first.\n" +
                "  copperd trigger safe-input-key --action setup --input text=<value>",
            );
            return;
          }
          await api.keyboard.typeText(text);
          break;
        }

        // ── setup ────────────────────────────────────────────────────────────
        // Stores the text in the OS keychain (Windows Credential Manager /
        // macOS Keychain / Linux SecretService). Accepts text via inputs.text,
        // which is passed with --input text=<value> from the CLI.
        case "setup": {
          const text = String(inputs.text ?? "");
          if (!text) {
            await api.notify(
              "Safe Input Key: provide the text to save.\n" +
                "  copperd trigger safe-input-key --action setup --input text=<value>",
            );
            return;
          }
          await api.secureStore.set(SERVICE, KEY_TEXT, text);
          await api.notify("Safe Input Key: text saved to OS keychain.");
          break;
        }

        // ── clear ─────────────────────────────────────────────────────────────
        // Removes the secret from the OS keychain.
        case "clear": {
          await api.secureStore.delete(SERVICE, KEY_TEXT);
          await api.notify("Safe Input Key: stored text cleared from OS keychain.");
          break;
        }

        default:
          await api.notify(`Safe Input Key: unknown action '${action}'.`);
      }
    },
  };
}

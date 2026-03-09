import type { Api } from "@host/api";

type Inputs = Record<string, unknown>;

function readAction(inputs: Inputs): string {
  return String(inputs.action ?? "status");
}

function summarize(inputs: Inputs) {
  return {
    taskbarAutoHide: Boolean(inputs.taskbarAutoHide ?? false),
    resolutionWidth: Number(inputs.resolutionWidth ?? 1920),
    resolutionHeight: Number(inputs.resolutionHeight ?? 1080),
    refreshRate: Number(inputs.refreshRate ?? 60),
    scalePercent: Number(inputs.scalePercent ?? 100)
  };
}

export default function (api: Api) {
  return {
    async onTrigger(inputs: Inputs = {}) {
      const action = readAction(inputs);
      const requested = summarize(inputs);

      await api.ui.show({
        type: "detail",
        title: "Windows Display Manager",
        content: {
          extensionId: "windows-display-manager",
          supportedActions: [
            "status",
            "toggle-taskbar-autohide",
            "set-taskbar-autohide",
            "set-resolution",
            "set-scale"
          ],
          requestedAction: action,
          requested,
          runFromDaemon: "copperd daemon trigger windows-display-manager --action <action-id>",
          note: "Daemon host API executes taskbar/resolution/scale actions, stores result in ~/.Copper/extensions/windows-display-manager/data.json, and drives the Windows tray icon."
        }
      });
      await api.notify(`windows-display-manager request queued: ${action}`);
    }
  };
}

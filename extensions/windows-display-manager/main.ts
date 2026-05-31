// Trigger: copperd daemon trigger windows-display-manager --action <action-id>
// Actions: status | toggle-taskbar-autohide | set-taskbar-autohide | set-resolution | set-scale

import type { Api } from "@host/api";

type Inputs = Record<string, unknown>;

export default function (api: Api) {
  return {
    async onTrigger(inputs: Inputs = {}) {
      const action = String(inputs.action ?? "status");

      if (!api.windows) {
        await api.notify("windows-display-manager: not available on this platform");
        return;
      }

      const display = api.windows.display;

      switch (action) {
        case "status": {
          const s = await display.status();
          await api.notify(
            `Display ${s.resolution.width}x${s.resolution.height}@${s.resolution.refreshRate}Hz` +
              ` | Scale ${s.scale.currentPercent}%` +
              ` | Taskbar auto-hide: ${s.taskbarAutoHide}`
          );
          break;
        }
        case "toggle-taskbar-autohide": {
          const r = await display.toggleTaskbarAutoHide();
          await api.notify(
            `Taskbar auto-hide: ${r.taskbarAutoHide ? "enabled" : "disabled"}`
          );
          break;
        }
        case "set-taskbar-autohide": {
          const autoHide = Boolean(inputs.autoHide ?? true);
          const r = await display.setTaskbarAutoHide(autoHide);
          await api.notify(
            `Taskbar auto-hide set to: ${r.taskbarAutoHide ? "enabled" : "disabled"}`
          );
          break;
        }
        case "set-resolution": {
          const width = Number(inputs.width ?? 1920);
          const height = Number(inputs.height ?? 1080);
          const refreshRate = Number(inputs.refreshRate ?? 60);
          await display.setResolution(width, height, refreshRate);
          await api.notify(`Resolution set to ${width}x${height}@${refreshRate}Hz`);
          break;
        }
        case "set-scale": {
          const scalePercent = Number(inputs.scalePercent ?? 100);
          await display.setScale(scalePercent);
          await api.notify(`Display scale set to ${scalePercent}%`);
          break;
        }
        default:
          await api.notify(`windows-display-manager: unknown action "${action}"`);
      }
    },
  };
}

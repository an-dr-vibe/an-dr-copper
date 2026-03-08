import type { Api } from "@host/api";

export default function (api: Api) {
  return {
    onLoad() {},

    async onTrigger(inputs: Record<string, unknown> = {}) {
      const folder = String(inputs.folder ?? "~/Downloads");
      const files = await api.fs.list(folder);
      await api.notify(`Found ${files.length} files in ${folder}`);

      await api.ui.show({
        type: "toast",
        message: "Sort Downloads completed"
      });
    },

    onUnload() {}
  };
}

import type { Api } from "@host/api";

const COUNTER_KEY = "session-counter/runs";

export default function (api: Api) {
  return {
    onLoad() {},

    async onTrigger() {
      const current = Number((await api.store.get<number>(COUNTER_KEY)) ?? 0);
      const next = current + 1;
      await api.store.set(COUNTER_KEY, next);

      await api.ui.show({
        type: "toast",
        message: `Session count: ${next}`
      });
    },

    onUnload() {}
  };
}

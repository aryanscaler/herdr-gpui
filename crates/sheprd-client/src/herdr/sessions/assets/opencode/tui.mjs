import { Plugin } from "@opencode/plugin/tui";
import { setup } from "./reporter.mjs";

export default Plugin.define({
  id: "sheprd.session-report",
  setup,
});

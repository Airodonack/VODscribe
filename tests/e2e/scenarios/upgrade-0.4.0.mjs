// Upgrading from 0.4.0: the library and settings that release left, with every kind
// of row it keeps (fixtures/upgrade-matrix.mjs says what is checked).

import { upgradeScenario } from "../fixtures/upgrade-matrix.mjs";

export default upgradeScenario("0.4.0", {
  from: 21,
  pieces: 3,
  bands: 3,
  paused: [[0, null, "paused", "last"]],
});

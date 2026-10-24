// Upgrading from 0.5.1: the library and settings that release left, with every kind
// of row it keeps (fixtures/upgrade-matrix.mjs says what is checked).

import { upgradeScenario } from "../fixtures/upgrade-matrix.mjs";

export default upgradeScenario("0.5.1", {
  from: 26,
  pieces: 5,
  bands: 4,
  paused: [[0, null, "paused", "last"]],
});

const test = require("node:test");
const assert = require("node:assert/strict");
const care = require("../src/shared/care.js");

const OPTIONS = { waterEveryMin: 50, moveEveryMin: 80 };
const DAYTIME = new Date(2026, 9, 28, 10, 0, 0).getTime();

test("a fresh cycle is quiet until the first interval", () => {
  const state = care.fresh(DAYTIME);
  assert.equal(care.dueType(state, DAYTIME + 49 * care.MINUTE, OPTIONS), null);
  assert.equal(care.dueType(state, DAYTIME + 50 * care.MINUTE, OPTIONS), "water");
});

test("the more overdue reminder wins", () => {
  const state = {
    waterAt: DAYTIME - 60 * care.MINUTE,
    moveAt: DAYTIME - 200 * care.MINUTE
  };
  assert.equal(care.dueType(state, DAYTIME, OPTIONS), "move");
});

test("sending one reminder postpones the other instead of stacking", () => {
  const state = {
    waterAt: DAYTIME - 60 * care.MINUTE,
    moveAt: DAYTIME - 200 * care.MINUTE
  };
  care.markSent(state, "water", DAYTIME, OPTIONS);
  assert.equal(state.waterAt, DAYTIME);
  assert.equal(state.moveAt, DAYTIME);
  assert.equal(care.dueType(state, DAYTIME + 49 * care.MINUTE, OPTIONS), null);
});

test("stays quiet at night and wakes up in the morning", () => {
  const night = new Date(2026, 9, 28, 23, 30, 0).getTime();
  const state = care.fresh(night - 5 * 60 * care.MINUTE);
  assert.equal(care.dueType(state, night, OPTIONS), null);

  const morning = new Date(2026, 9, 29, 8, 0, 0).getTime();
  assert.equal(care.dueType(state, morning, OPTIONS), "water");
});

test("normalize discards old, future or broken timestamps", () => {
  const state = care.normalize(
    { waterAt: DAYTIME - 7 * 60 * care.MINUTE, moveAt: "nope" },
    DAYTIME
  );
  assert.equal(state.waterAt, DAYTIME);
  assert.equal(state.moveAt, DAYTIME);
  assert.deepEqual(care.normalize(null, DAYTIME), care.fresh(DAYTIME));
});

test("intervals fall back to sane defaults", () => {
  const options = care.options({ waterEveryMin: 0, moveEveryMin: 9999 });
  assert.equal(options.waterMs, 50 * care.MINUTE);
  assert.equal(options.moveMs, 240 * care.MINUTE);
});

test("lines are short, warm and non-empty", () => {
  for (const type of ["water", "move"]) {
    const line = care.line(type, () => 0);
    assert.ok(line.length > 8, line);
    assert.ok(!line.includes("<"), line);
  }
});

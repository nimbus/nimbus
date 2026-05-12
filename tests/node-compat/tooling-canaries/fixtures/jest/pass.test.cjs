test("jest canary pass", () => {
  expect([1, 2, 3].map((value) => value * 2)).toEqual([2, 4, 6]);
});


type Payload = {
  value: number;
};

const payload: Payload = {
  value: 42,
};

console.log(`tsx-ok:${payload.value * 2}`);


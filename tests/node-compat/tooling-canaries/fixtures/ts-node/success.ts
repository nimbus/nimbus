interface Payload {
  readonly value: number;
}

const payload: Payload = {
  value: 21,
};

console.log(`ts-node-ok:${payload.value * 2}`);


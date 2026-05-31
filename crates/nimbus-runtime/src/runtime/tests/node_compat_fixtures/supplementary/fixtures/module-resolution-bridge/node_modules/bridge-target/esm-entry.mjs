import feature from 'bridge-target/feature';

export const exportedKind = 'esm-entry';
export const featureKind = feature;

export default {
  entry: exportedKind,
  feature,
};

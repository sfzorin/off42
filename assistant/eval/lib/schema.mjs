// Minimal JSON Schema check for the subset used in assistant/tools/*.json.
export function validate(schema, value, path = '$') {
  const errs = [];
  const types = schema.type ? [].concat(schema.type) : null;
  const typeOf = (v) => (v === null ? 'null' : Array.isArray(v) ? 'array' : Number.isInteger(v) ? 'integer' : typeof v);
  if (types) {
    const t = typeOf(value);
    const ok = types.some((x) => x === t || (x === 'number' && t === 'integer'));
    if (!ok) return [`${path}: expected ${types.join('|')}, got ${t}`];
  }
  if (schema.enum && !schema.enum.includes(value)) errs.push(`${path}: must be one of ${schema.enum.join(', ')}`);
  if (typeof value === 'string' && schema.minLength && value.length < schema.minLength) errs.push(`${path}: too short`);
  if (typeof value === 'number') {
    if (schema.minimum !== undefined && value < schema.minimum) errs.push(`${path}: below ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) errs.push(`${path}: above ${schema.maximum}`);
  }
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems) errs.push(`${path}: fewer than ${schema.minItems} items`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems) errs.push(`${path}: more than ${schema.maxItems} items`);
    if (schema.items) value.forEach((v, i) => errs.push(...validate(schema.items, v, `${path}[${i}]`)));
  }
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    for (const r of schema.required || []) if (!(r in value)) errs.push(`${path}: missing required "${r}"`);
    for (const [k, v] of Object.entries(value)) {
      if (schema.properties && schema.properties[k]) errs.push(...validate(schema.properties[k], v, `${path}.${k}`));
      else if (schema.additionalProperties === false) errs.push(`${path}: unknown property "${k}"`);
    }
  }
  return errs;
}

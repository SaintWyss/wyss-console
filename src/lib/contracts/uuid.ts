declare const uuidBrand: unique symbol;
export type Uuid = string & { readonly [uuidBrand]: true };

/** Retains wire case; no UUID version or variant is prescribed by v1. */
export function parseUuid(value: unknown): Uuid {
  if (
    typeof value !== 'string' ||
    value.length !== 36 ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      value,
    )
  ) {
    throw new Error('uuid: invalid_uuid');
  }
  return value as Uuid;
}

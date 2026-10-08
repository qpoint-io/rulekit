// Rulekit spans are UTF-8 byte offsets; the DOM speaks UTF-16 code units.
// For ASCII sources both are identical, so the common case is free.

export type OffsetMap = {
  toUtf16(byte: number): number
  toByte(utf16: number): number
}

const identity: OffsetMap = { toUtf16: (b) => b, toByte: (u) => u }

export function offsetMap(source: string): OffsetMap {
  // eslint-disable-next-line no-control-regex
  if (/^[\x00-\x7f]*$/.test(source)) return identity

  // byteAt[i] = UTF-8 offset where UTF-16 unit i starts.
  const byteAt = new Uint32Array(source.length + 1)
  let byte = 0
  for (let i = 0; i < source.length; i++) {
    byteAt[i] = byte
    const code = source.charCodeAt(i)
    if (code < 0x80) byte += 1
    else if (code < 0x800) byte += 2
    else if (code >= 0xd800 && code <= 0xdbff && i + 1 < source.length) {
      byteAt[++i] = byte
      byte += 4
    } else byte += 3
  }
  byteAt[source.length] = byte

  return {
    toByte: (u) => byteAt[Math.max(0, Math.min(u, source.length))],
    toUtf16: (b) => {
      let lo = 0
      let hi = source.length
      while (lo < hi) {
        const mid = (lo + hi) >> 1
        if (byteAt[mid] < b) lo = mid + 1
        else hi = mid
      }
      return lo
    },
  }
}

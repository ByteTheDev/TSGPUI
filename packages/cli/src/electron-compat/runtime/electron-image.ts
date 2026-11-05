const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function encode(bytes: Uint8Array) {
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] || 0) << 8) | (bytes[i + 2] || 0);
    out += alphabet[n >> 18] + alphabet[(n >> 12) & 63] + (i + 1 < bytes.length ? alphabet[(n >> 6) & 63] : '=') + (i + 2 < bytes.length ? alphabet[n & 63] : '=');
  }
  return out;
}
function decode(value: string) {
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) throw new TypeError('Invalid base64 image');
  const out: number[] = []; let bits = 0, acc = 0;
  for (const c of value) { if (c === '=') break; acc = (acc << 6) | alphabet.indexOf(c); bits += 6; if (bits >= 8) { bits -= 8; out.push((acc >> bits) & 255); } }
  return new Uint8Array(out);
}
function dimensions(bytes: Uint8Array) {
  const magic = [137,80,78,71,13,10,26,10];
  if (bytes.length < 33 || magic.some((v,i) => bytes[i] !== v) || bytes[8] !== 0 || bytes[9] !== 0 || bytes[10] !== 0 || bytes[11] !== 13 || bytes[12] !== 73 || bytes[13] !== 72 || bytes[14] !== 68 || bytes[15] !== 82) return null;
  const width = ((bytes[16] * 0x1000000) + (bytes[17] << 16) + (bytes[18] << 8) + bytes[19]);
  const height = ((bytes[20] * 0x1000000) + (bytes[21] << 16) + (bytes[22] << 8) + bytes[23]);
  return width && height ? {width, height} : null;
}
class NativeImage {
  private bytes: Uint8Array;
  private size: {width: number, height: number} | null;
  constructor(bytes: Uint8Array) { this.bytes = new Uint8Array(bytes); this.size = dimensions(this.bytes); if (!this.size) this.bytes = new Uint8Array(0); }
  isEmpty() { return !this.size; }
  getSize(_scaleFactor?: number) { return this.size ? {...this.size} : {width: 0, height: 0}; }
  toPNG(_options?: any) { return new Uint8Array(this.bytes); }
  toDataURL(_options?: any) { return `data:image/png;base64,${encode(this.bytes)}`; }
  getScaleFactors() { return this.isEmpty() ? [] : [1]; }
  toJPEG() { throw new Error('nativeImage.toJPEG is not supported'); }
  toBitmap() { throw new Error('nativeImage.toBitmap is not supported'); }
  crop() { throw new Error('nativeImage.crop is not supported'); }
  resize() { throw new Error('nativeImage.resize is not supported'); }
}
export const nativeImage = {
  createEmpty() { return new NativeImage(new Uint8Array(0)); },
  createFromBuffer(buffer: Uint8Array) {
    if (!(buffer instanceof Uint8Array)) throw new TypeError('nativeImage.createFromBuffer requires a Uint8Array');
    return new NativeImage(buffer);
  },
  createFromDataURL(url: string) {
    if (typeof url !== 'string') throw new TypeError('nativeImage.createFromDataURL requires a string');
    const prefix = 'data:image/png;base64,';
    if (!url.startsWith(prefix)) throw new Error('nativeImage currently supports PNG data URLs only');
    return new NativeImage(decode(url.slice(prefix.length)));
  },
  createFromPath() { throw new Error('nativeImage.createFromPath is not supported'); },
  createFromBitmap() { throw new Error('nativeImage.createFromBitmap is not supported'); },
};

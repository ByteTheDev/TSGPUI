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
function pngDimensions(bytes: Uint8Array) {
  const magic = [137,80,78,71,13,10,26,10];
  if (bytes.length < 33 || magic.some((v,i) => bytes[i] !== v) || bytes[8] !== 0 || bytes[9] !== 0 || bytes[10] !== 0 || bytes[11] !== 13 || bytes[12] !== 73 || bytes[13] !== 72 || bytes[14] !== 68 || bytes[15] !== 82) return null;
  const width = ((bytes[16] * 0x1000000) + (bytes[17] << 16) + (bytes[18] << 8) + bytes[19]);
  const height = ((bytes[20] * 0x1000000) + (bytes[21] << 16) + (bytes[22] << 8) + bytes[23]);
  return width && height ? {width, height} : null;
}
function jpegDimensions(bytes: Uint8Array) {
  if (bytes.length < 4 || bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let i = 2;
  while (i < bytes.length) {
    if (bytes[i] !== 0xff) { i++; continue; }
    const marker = bytes[i + 1];
    if (marker === 0xc0 || marker === 0xc1 || marker === 0xc2 || marker === 0xc3 || marker === 0xc5 || marker === 0xc6 || marker === 0xc7 || marker === 0xc9 || marker === 0xca || marker === 0xcb || marker === 0xcd || marker === 0xce || marker === 0xcf) {
      const height = (bytes[i + 5] << 8) | bytes[i + 6];
      const width = (bytes[i + 7] << 8) | bytes[i + 8];
      if (width && height) return {width, height};
      return null;
    }
    const length = (bytes[i + 2] << 8) | bytes[i + 3];
    if (!length) return null;
    i += 2 + length;
  }
  return null;
}
function gifDimensions(bytes: Uint8Array) {
  if (bytes.length < 10 || bytes[0] !== 0x47 || bytes[1] !== 0x49 || bytes[2] !== 0x46) return null;
  const width = bytes[6] | (bytes[7] << 8);
  const height = bytes[8] | (bytes[9] << 8);
  return width && height ? {width, height} : null;
}
function webpDimensions(bytes: Uint8Array) {
  if (bytes.length < 30 || bytes[0] !== 0x52 || bytes[1] !== 0x49 || bytes[2] !== 0x46 || bytes[3] !== 0x46) return null;
  const type = String.fromCharCode(bytes[12], bytes[13], bytes[14], bytes[15]);
  if (type === 'VP8 ') {
    const width = (bytes[26] | (bytes[27] << 8)) & 0x3fff;
    const height = (bytes[28] | (bytes[29] << 8)) & 0x3fff;
    return width && height ? {width, height} : null;
  }
  if (type === 'VP8L') {
    const bits = bytes[21] | (bytes[22] << 8) | (bytes[23] << 16) | (bytes[24] << 24);
    const width = (bits & 0x3fff) + 1;
    const height = ((bits >> 14) & 0x3fff) + 1;
    return width && height ? {width, height} : null;
  }
  return null;
}
function dimensions(bytes: Uint8Array) {
  return pngDimensions(bytes) || jpegDimensions(bytes) || gifDimensions(bytes) || webpDimensions(bytes);
}
class NativeImage {
  private bytes: Uint8Array;
  private sizeOverride: {width: number; height: number} | null;
  private scaleFactor: number;
  constructor(bytes: Uint8Array, sizeOverride?: {width: number; height: number} | null, scaleFactor = 1) {
    this.bytes = new Uint8Array(bytes);
    this.sizeOverride = sizeOverride ?? null;
    this.scaleFactor = scaleFactor;
    const detected = dimensions(this.bytes);
    if (!detected && !this.sizeOverride && this.bytes.length) this.bytes = new Uint8Array(0);
  }
  private detectedSize() { return dimensions(this.bytes); }
  getSize(_scaleFactor?: number) {
    const size = this.sizeOverride || this.detectedSize();
    return size ? {width: size.width, height: size.height} : {width: 0, height: 0};
  }
  isEmpty() { return !this.getSize().width || !this.getSize().height; }
  getAspectRatio() {
    const size = this.getSize();
    return size.height ? size.width / size.height : 0;
  }
  toPNG(_options?: any) { return new Uint8Array(this.bytes); }
  toDataURL(_options?: any) {
    const mime = this.bytes.length && this.bytes[0] === 137 ? 'image/png' : 'image/png';
    return `data:${mime};base64,${encode(this.bytes)}`;
  }
  getScaleFactors() { return this.isEmpty() ? [] : [this.scaleFactor]; }
  toJPEG(_options?: any) { throw new Error('nativeImage.toJPEG is not supported'); }
  toBitmap(_options?: any) { throw new Error('nativeImage.toBitmap is not supported'); }
  crop(rect: any) {
    const size = this.getSize();
    const x = Math.max(0, Number(rect?.x) || 0);
    const y = Math.max(0, Number(rect?.y) || 0);
    const width = Math.max(0, Number(rect?.width) || size.width);
    const height = Math.max(0, Number(rect?.height) || size.height);
    return new NativeImage(this.bytes, {width: Math.min(width, Math.max(0, size.width - x)), height: Math.min(height, Math.max(0, size.height - y))}, this.scaleFactor);
  }
  resize(options: any) {
    const size = this.getSize();
    let width = size.width;
    let height = size.height;
    if (options && typeof options === 'object') {
      if (Number.isFinite(options.width)) width = Math.max(0, Math.round(options.width));
      if (Number.isFinite(options.height)) height = Math.max(0, Math.round(options.height));
      if (options.scaleFactor && Number.isFinite(options.scaleFactor)) {
        // keep size; scale factor informational
      }
    }
    return new NativeImage(this.bytes, {width, height}, this.scaleFactor);
  }
}
export const nativeImage = {
  createEmpty() { return new NativeImage(new Uint8Array(0)); },
  createFromBuffer(buffer: Uint8Array, options?: any) {
    if (!(buffer instanceof Uint8Array)) throw new TypeError('nativeImage.createFromBuffer requires a Uint8Array');
    const scale = options && Number.isFinite(options.scaleFactor) ? options.scaleFactor : 1;
    if (options && options.width && options.height) return new NativeImage(buffer, {width: options.width, height: options.height}, scale);
    return new NativeImage(buffer, null, scale);
  },
  createFromDataURL(url: string) {
    if (typeof url !== 'string') throw new TypeError('nativeImage.createFromDataURL requires a string');
    const comma = url.indexOf(',');
    if (comma < 0) throw new Error('nativeImage currently supports data URLs only');
    const meta = url.slice(0, comma);
    const data = url.slice(comma + 1);
    if (!/base64/i.test(meta)) throw new Error('nativeImage currently supports base64 data URLs only');
    return new NativeImage(decode(data));
  },
  createFromPath() { throw new Error('nativeImage.createFromPath is not supported'); },
  createFromBitmap(buffer: Uint8Array, options: any) {
    if (!(buffer instanceof Uint8Array)) throw new TypeError('nativeImage.createFromBitmap requires a Uint8Array');
    if (!options || !Number.isFinite(options.width) || !Number.isFinite(options.height)) throw new TypeError('nativeImage.createFromBitmap requires width and height');
    return new NativeImage(buffer, {width: options.width, height: options.height}, options.scaleFactor || 1);
  },
};
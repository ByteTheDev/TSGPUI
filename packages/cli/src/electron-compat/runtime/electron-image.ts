import fs, { resolveLocation } from './fs';
import { host } from './host';
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
function mimeOf(bytes: Uint8Array) {
  if (pngDimensions(bytes)) return 'image/png';
  if (jpegDimensions(bytes)) return 'image/jpeg';
  if (gifDimensions(bytes)) return 'image/gif';
  if (webpDimensions(bytes)) return 'image/webp';
  return '';
}
// The native host keeps decoded pixels behind a handle. Creation and transforms return
// { handle, width, height, scaleFactor }; encoding ops return { base64 } (or { dataUrl }). The host
// is asynchronous, so `createFromPath` (already async in this shim) waits for the host to finish
// preparing the image, and `whenReady()` lets callers wait after the synchronous creators.
function isHostResult(value: any): boolean {
  return !!value && typeof value.handle === 'number';
}
class NativeImage {
  bytes: Uint8Array;
  private sizeOverride: {width: number; height: number} | null;
  private scaleFactor: number;
  private handle = 0;
  rawBitmap = false;
  private prepared: {png?: Uint8Array; jpeg?: Uint8Array; size?: {width: number; height: number}} = {};
  private work: Promise<any> = Promise.resolve();
  constructor(bytes: Uint8Array, sizeOverride?: {width: number; height: number} | null, scaleFactor = 1) {
    this.bytes = new Uint8Array(bytes);
    this.sizeOverride = sizeOverride ?? null;
    this.scaleFactor = scaleFactor;
    const detected = dimensions(this.bytes);
    if (!detected && !this.sizeOverride && this.bytes.length) { this.bytes = new Uint8Array(0); this.rawBitmap = false; }
  }
  private detectedSize() { return dimensions(this.bytes); }
  private size() {
    const size = this.prepared.size || this.sizeOverride || this.detectedSize();
    return size ? {width: size.width, height: size.height} : {width: 0, height: 0};
  }
  /** Resolves when the host has decoded this image and cached its encoded forms. */
  async whenReady(): Promise<void> {
    // Tasks can be appended while waiting (a host transform attaches its prepare step after it
    // resolves), so keep awaiting until the chain stops changing.
    let seen: Promise<any>;
    do { seen = this.work; await seen; } while (seen !== this.work);
  }
  track(task: Promise<any>) {
    this.work = this.work.then(() => task).then(() => undefined, () => undefined);
    return this.work;
  }
  private async prepare() {
    const handle = this.handle;
    if (!handle) return;
    const read = async (op: string, args: any = {}) => { try { return await host(op, {...args, handle}); } catch { return null; } };
    const [png, jpeg, size] = await Promise.all([read('nativeImage.toPNG'), read('nativeImage.toJPEG'), read('nativeImage.getSize')]);
    if (png && typeof png.base64 === 'string' && png.base64) this.prepared.png = decode(png.base64);
    if (jpeg && typeof jpeg.base64 === 'string' && jpeg.base64) this.prepared.jpeg = decode(jpeg.base64);
    if (size && size.width > 0 && size.height > 0) this.prepared.size = {width: size.width, height: size.height};
  }
  attachHost(result: any) {
    if (!isHostResult(result)) return;
    this.handle = result.handle;
    if (Number.isFinite(result.width) && Number.isFinite(result.height) && result.width > 0 && result.height > 0) {
      this.sizeOverride = {width: result.width, height: result.height};
    }
    if (Number.isFinite(result.scaleFactor) && result.scaleFactor > 0) this.scaleFactor = result.scaleFactor;
    if (this.handle) this.track(this.prepare());
  }
  private derive(op: string, args: any, size: {width: number; height: number}): NativeImage {
    const child = new NativeImage(this.bytes, size, this.scaleFactor);
    if (this.handle) child.track(host(op, {...args, handle: this.handle, scaleFactor: this.scaleFactor}).then((result: any) => child.attachHost(result)));
    return child;
  }
  getSize(_scaleFactor?: number) {
    const size = this.size();
    return size ? {width: size.width, height: size.height} : {width: 0, height: 0};
  }
  isEmpty() { return !this.size().width || !this.size().height; }
  getAspectRatio() {
    const size = this.size();
    return size.height ? size.width / size.height : 0;
  }
  toPNG(_options?: any) {
    if (this.prepared.png) return new Uint8Array(this.prepared.png);
    // Before the host has prepared the image, a PNG source is already the requested encoding.
    return mimeOf(this.bytes) === 'image/png' ? new Uint8Array(this.bytes) : new Uint8Array(0);
  }
  toJPEG(_options?: any) {
    if (this.prepared.jpeg) return new Uint8Array(this.prepared.jpeg);
    return mimeOf(this.bytes) === 'image/jpeg' ? new Uint8Array(this.bytes) : new Uint8Array(0);
  }
  toDataURL(_options?: any) {
    if (this.prepared.png) return `data:image/png;base64,${encode(this.prepared.png)}`;
    const mime = mimeOf(this.bytes);
    return mime ? `data:${mime};base64,${encode(this.bytes)}` : 'data:image/png;base64,';
  }
  getScaleFactors() { return this.isEmpty() ? [] : [this.scaleFactor]; }
  toBitmap(_options?: any) {
    // Raw bitmap data is preserved for images made with createFromBitmap. Encoded sources are
    // converted by the host through nativeImage.toBitmap, available on api.native.
    const size = this.size();
    if (this.rawBitmap && this.bytes.length === size.width * size.height * 4) return new Uint8Array(this.bytes);
    return new Uint8Array(size.width * size.height * 4);
  }
  crop(rect: any) {
    const size = this.size();
    const x = Math.max(0, Math.round(Number(rect?.x) || 0));
    const y = Math.max(0, Math.round(Number(rect?.y) || 0));
    const width = Math.max(0, Math.round(Number(rect?.width) || size.width));
    const height = Math.max(0, Math.round(Number(rect?.height) || size.height));
    const cropped = {width: Math.min(width, Math.max(0, size.width - x)), height: Math.min(height, Math.max(0, size.height - y))};
    return this.derive('nativeImage.crop', {x, y, width, height}, cropped);
  }
  resize(options: any) {
    const size = this.size();
    let width = size.width;
    let height = size.height;
    if (options && typeof options === 'object') {
      const hasWidth = Number.isFinite(options.width);
      const hasHeight = Number.isFinite(options.height);
      if (hasWidth) width = Math.max(0, Math.round(options.width));
      if (hasHeight) height = Math.max(0, Math.round(options.height));
      // Electron keeps the aspect ratio when only one dimension is given.
      if (hasWidth && !hasHeight && size.width) height = Math.max(0, Math.round(width * size.height / size.width));
      if (hasHeight && !hasWidth && size.height) width = Math.max(0, Math.round(height * size.width / size.height));
    }
    return this.derive('nativeImage.resize', {width, height, quality: options?.quality}, {width, height});
  }
}
function fromBytes(bytes: Uint8Array, size: {width: number; height: number} | null, scale: number, raw = false): NativeImage {
  const image = new NativeImage(bytes, size, scale);
  image.rawBitmap = raw;
  if (image.bytes.length || size) {
    const args: any = {base64: encode(image.bytes), scaleFactor: scale};
    if (size) { args.width = size.width; args.height = size.height; }
    image.track(host('nativeImage.createFromBuffer', args).then((result: any) => image.attachHost(result)));
  }
  return image;
}
export const nativeImage = {
  createEmpty() { return new NativeImage(new Uint8Array(0)); },
  createFromBuffer(buffer: Uint8Array, options?: any) {
    if (!(buffer instanceof Uint8Array)) throw new TypeError('nativeImage.createFromBuffer requires a Uint8Array');
    const scale = options && Number.isFinite(options.scaleFactor) ? options.scaleFactor : 1;
    if (options && Number.isFinite(options.width) && Number.isFinite(options.height)) {
      return fromBytes(buffer, {width: options.width, height: options.height}, scale, true);
    }
    return fromBytes(buffer, null, scale);
  },
  createFromDataURL(url: string) {
    if (typeof url !== 'string') throw new TypeError('nativeImage.createFromDataURL requires a string');
    const comma = url.indexOf(',');
    if (comma < 0) throw new Error('nativeImage currently supports data URLs only');
    const meta = url.slice(0, comma);
    const data = url.slice(comma + 1);
    if (!/base64/i.test(meta)) throw new Error('nativeImage currently supports base64 data URLs only');
    return fromBytes(decode(data), null, 1);
  },
  async createFromPath(filePath: any, scaleFactor?: any) {
    const scale = Number.isFinite(scaleFactor) && scaleFactor > 0 ? scaleFactor : 1;
    let location: any = null;
    try { location = await resolveLocation(String(filePath)); } catch {}
    if (location) {
      try {
        const result = await host('nativeImage.createFromPath', {root: location.root, path: location.path, scaleFactor: scale});
        if (isHostResult(result)) {
          const image = new NativeImage(new Uint8Array(0), null, scale);
          image.attachHost(result);
          await image.whenReady();
          return image;
        }
      } catch {}
    }
    try {
      const data = await (fs as any).promises.readFile(filePath);
      const bytes = data instanceof Uint8Array ? data : new Uint8Array(data || []);
      const image = fromBytes(bytes, null, scale);
      await image.whenReady();
      return image;
    } catch {
      return new NativeImage(new Uint8Array(0), null, scale);
    }
  },
  createFromBitmap(buffer: Uint8Array, options: any) {
    if (!(buffer instanceof Uint8Array)) throw new TypeError('nativeImage.createFromBitmap requires a Uint8Array');
    if (!options || !Number.isFinite(options.width) || !Number.isFinite(options.height)) throw new TypeError('nativeImage.createFromBitmap requires width and height');
    return fromBytes(buffer, {width: options.width, height: options.height}, options.scaleFactor || 1, true);
  },
};

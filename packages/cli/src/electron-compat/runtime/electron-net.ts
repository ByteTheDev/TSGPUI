import { request as httpsRequest } from './https';

// This uses the host's permission-checked HTTPS transport. Unlike Electron's
// Chromium-backed net module it currently buffers UTF-8 text, has no cookie
// session or redirects, and cannot stream or cancel an in-flight request.
export const net = {
  request(options: any) {
    if (typeof options !== 'string' && (!options || typeof options !== 'object')) throw new TypeError('net.request requires a URL or options object');
    if (typeof options === 'object') {
      if (options.session || options.partition || options.useSessionCookies || options.credentials) throw new Error('net.request session and credential options are not supported');
      if (options.redirect && options.redirect !== 'manual') throw new Error('net.request redirect following is not supported');
    }
    const req = httpsRequest(options);
    const write = req.write, end = req.end;
    req.write = (chunk: any) => { if (typeof chunk !== 'string') throw new TypeError('net.request supports text bodies only'); return write(chunk); };
    req.end = (chunk?: any) => { if (chunk !== undefined && typeof chunk !== 'string') throw new TypeError('net.request supports text bodies only'); return end(chunk); };
    return req;
  },
  fetch() { return Promise.reject(new Error('net.fetch is not supported')); },
  isOnline() { throw new Error('net.isOnline is not supported'); },
};

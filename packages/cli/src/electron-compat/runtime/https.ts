import { host } from './host';
import { EventEmitter } from './events';

function target(input: any): {url: string, method: string, headers: any} {
  if (typeof input === 'string') return {url: input, method: 'GET', headers: {}};
  const o = input || {};
  const protocol = o.protocol || 'https:';
  const hostname = o.hostname || o.host || 'localhost';
  const port = o.port ? `:${o.port}` : '';
  return {url: o.url || o.href || `${protocol}//${hostname}${port}${o.path || '/'}`, method: o.method || 'GET', headers: o.headers || {}};
}
export function request(input: any, callback?: Function) {
  const options = target(input), req = new EventEmitter() as any;
  let body = '', ended = false;
  options.headers = {...options.headers};
  req.setHeader = (name: string, value: any) => {
    if (ended) throw new Error('Cannot set headers after end');
    if (typeof value !== 'string') throw new TypeError('Only string header values are supported');
    const old = Object.keys(options.headers).find(k => k.toLowerCase() === name.toLowerCase());
    if (old) delete options.headers[old];
    options.headers[name] = value;
  };
  req.getHeader = (name: string) => { const key = Object.keys(options.headers).find(k => k.toLowerCase() === name.toLowerCase()); return key === undefined ? undefined : options.headers[key]; };
  req.removeHeader = (name: string) => { if (ended) throw new Error('Cannot remove headers after end'); const key = Object.keys(options.headers).find(k => k.toLowerCase() === name.toLowerCase()); if (key !== undefined) delete options.headers[key]; };
  req.write = (chunk: any) => { if (ended) throw new Error('write after end'); body += String(chunk); return true; };
  req.end = (chunk?: any) => {
    if (ended) throw new Error('end after end');
    if (chunk !== undefined) req.write(chunk);
    ended = true;
    host('net.request', {...options, body}).then(result => {
      const response = new EventEmitter() as any;
      response.statusCode = result.status; response.headers = result.headers;
      if (callback) callback(response);
      req.emit('response', response);
      if (result.body) response.emit('data', result.body);
      response.emit('end');
    }, error => { req.emit('error', error); });
    return req;
  };
  return req;
}
export function get(input: any, callback?: Function) { const req = request(input, callback); req.end(); return req; }
export default {get, request};

import { host } from './host';
import { EventEmitter } from './events';

function target(input: any): {url: string, method: string, headers: any} {
  if (typeof input === 'string') return {url: input, method: 'GET', headers: {}};
  const o = input || {};
  const protocol = o.protocol || 'https:';
  const hostname = o.hostname || o.host || 'localhost';
  const port = o.port ? `:${o.port}` : '';
  return {url: o.href || `${protocol}//${hostname}${port}${o.path || '/'}`, method: o.method || 'GET', headers: o.headers || {}};
}
export function request(input: any, callback?: Function) {
  const options = target(input), req = new EventEmitter() as any;
  let body = '', ended = false;
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

import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareWebviewHtml } from '../src/webview.mjs';

for (const hosts of [[], ['api.example.com']]) {
  test(`WebView CSP for ${hosts.length ? 'a network host' : 'no network hosts'}`, async () => {
    const html = await prepareWebviewHtml('<html><head><script>window.ready=true</script></head><body></body></html>', '', {backendJs:'', title:'Test', networkHosts:hosts});
    const nonce = /<script nonce="([^"]+)"/.exec(html)[1];
    const policy = /content="([^"]+)"/.exec(html)[1].replaceAll('&#39;', "'");
    assert.equal(policy, `default-src 'none'; script-src 'self' 'nonce-${nonce}' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data:; connect-src 'self'${hosts.map(host => ' https://' + host).join('')}; form-action 'self'; frame-src 'self'; object-src 'none'; base-uri 'self'`);
    assert.ok(html.includes(`<script data-tsgpui="backend" nonce="${nonce}">`));
    const needsEval = html.includes('new Function(...hidden,source)');
    if (!needsEval) assert.ok(!policy.includes("'unsafe-eval'"), 'unsafe-eval must be removed when the bootstrap no longer needs it');
    assert.ok(!policy.includes("script-src 'self' 'unsafe-inline'"));
  });
}

import { extractAccountObservation } from './observation.js';

export function validateNativeStatus(response, observation) {
  if (!response || response.ok !== true || Object.keys(response).length !== 2
      || !Object.hasOwn(response, 'status')) return false;
  const status = response.status;
  if (!status || typeof status !== 'object' || Array.isArray(status)) return false;
  const keys = ['configured', 'mode', 'state', 'username', 'live_authenticated',
    'account_observed', 'fresh', 'automation_permission', 'route_assurance',
    'observed_at', 'verification_ttl_seconds'];
  if (Object.keys(status).length !== keys.length
      || keys.some((key) => !Object.hasOwn(status, key))) return false;
  if (status.configured !== true || status.mode !== 'extension'
      || status.state !== 'account_observed' || status.username !== observation.username
      || status.live_authenticated !== false || status.account_observed !== true
      || status.fresh !== true || status.automation_permission !== 'not_verified'
      || status.route_assurance !== 'operator_managed_unverified'
      || status.verification_ttl_seconds !== 300 || typeof status.observed_at !== 'string'
      || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(status.observed_at)) return false;
  const age = Date.now() - Date.parse(status.observed_at);
  return Number.isFinite(age) && age >= -5000 && age < 300_000;
}

const connectButton = document.getElementById('connect');
const statusText = document.getElementById('status');
const refused = 'Account observation refused; check pairing, account and Fiverr page.';
let cancelConnection = null;
let busy = false;
let closed = false;

function sendObservation(observation) {
  return new Promise((resolve, reject) => {
    let port = null;
    let timer = null;
    let settled = false;
    const finish = (response, error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      cancelConnection = null;
      if (port) {
        port.onMessage.removeListener(onMessage);
        port.onDisconnect.removeListener(onDisconnect);
        try { port.disconnect(); } catch { /* Already disconnected. */ }
      }
      if (error) reject(new Error(error));
      else resolve(response);
    };
    const onMessage = (response) => finish(response, null);
    const onDisconnect = () => {
      // Consume browser diagnostics without exposing native paths or host messages.
      void chrome.runtime.lastError;
      finish(null, refused);
    };
    cancelConnection = () => finish(null, 'Observation cancelled. Try again with the popup open.');
    timer = setTimeout(() => finish(null, 'No native-host response within 10 seconds. Check installation and pairing.'), 10_000);
    try {
      port = chrome.runtime.connectNative('com.fiverr_agent_mcp.session');
      port.onMessage.addListener(onMessage);
      port.onDisconnect.addListener(onDisconnect);
      port.postMessage(observation);
    } catch {
      finish(null, refused);
    }
  });
}

connectButton.addEventListener('click', async () => {
  if (busy || closed) return;
  busy = true;
  connectButton.disabled = true;
  statusText.textContent = 'Checking this tab. Keep the popup open…';
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab || !Number.isInteger(tab.id) || !tab.url) throw new Error(refused);
    const url = new URL(tab.url);
    if (url.protocol !== 'https:' || url.hostname !== 'www.fiverr.com' || url.port !== ''
        || url.username !== '' || url.password !== '' || url.search !== '' || url.hash !== ''
        || !/^\/users\/(?:[A-Za-z0-9_-]{1,80}\/)?manage_gigs\/?$/.test(url.pathname)) {
      throw new Error(refused);
    }
    const results = await chrome.scripting.executeScript({
      target: { tabId: tab.id, allFrames: false },
      world: 'ISOLATED',
      func: extractAccountObservation,
    });
    const observation = results.length === 1 && results[0].frameId === 0
      ? results[0].result : null;
    if (closed || !observation || observation.type !== 'account_observation'
        || observation.url !== tab.url || typeof observation.username !== 'string'
        || !/^[A-Za-z0-9_-]{1,80}$/.test(observation.username)
        || observation.management !== true || observation.logout !== true
        || Object.keys(observation).length !== 5) throw new Error(refused);
    statusText.textContent = 'Waiting for the paired local app…';
    const response = await sendObservation(observation);
    if (!validateNativeStatus(response, observation)) throw new Error(refused);
    statusText.textContent = `Account ${observation.username} observed by the paired app. Not OAuth or independently authenticated access. Observation expires after 5 minutes; routing and automation permission remain unverified.`;
  } catch {
    // Both DOM and native replies are untrusted; render only fixed errors.
    statusText.textContent = refused;
  } finally {
    busy = false;
    connectButton.disabled = false;
  }
});

window.addEventListener('pagehide', () => {
  closed = true;
  if (cancelConnection) cancelConnection();
});

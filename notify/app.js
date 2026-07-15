const els = {
  form: document.querySelector('#connectionForm'),
  apiUrl: document.querySelector('#apiUrl'),
  apiKey: document.querySelector('#apiKey'),
  revealKey: document.querySelector('#revealKey'),
  connectButton: document.querySelector('#connectButton'),
  connectLabel: document.querySelector('#connectLabel'),
  disconnectButton: document.querySelector('#disconnectButton'),
  feedback: document.querySelector('#feedback'),
  installNote: document.querySelector('#installNote'),
  liveState: document.querySelector('#liveState'),
  liveLabel: document.querySelector('#liveLabel'),
  signalList: document.querySelector('#signalList'),
  emptyLog: document.querySelector('#emptyLog'),
  clearLog: document.querySelector('#clearLog'),
};

const STORAGE = {
  apiUrl: 'stardive.relay.apiUrl',
  apiKey: 'stardive.relay.apiKey',
  subscriptionId: 'stardive.relay.subscriptionId',
};

const isStandalone = window.matchMedia('(display-mode: standalone)').matches || window.navigator.standalone === true;
els.installNote.hidden = isStandalone;
els.apiUrl.value = localStorage.getItem(STORAGE.apiUrl) || els.apiUrl.value;
els.apiKey.value = localStorage.getItem(STORAGE.apiKey) || '';

function authHeaders(apiKey, json = false) {
  const headers = {};
  if (json) headers['Content-Type'] = 'application/json';
  if (apiKey) headers.Authorization = `Bearer ${apiKey}`;
  return headers;
}

function endpoint(path) {
  return `${els.apiUrl.value.trim().replace(/\/$/, '')}/v1/notifications${path}`;
}

function setFeedback(message, type = '') {
  els.feedback.textContent = message;
  els.feedback.className = `feedback ${type}`.trim();
}

function setConnected(connected) {
  els.liveState.classList.toggle('connected', connected);
  els.liveLabel.textContent = connected ? 'RECEIVING' : 'OFFLINE';
  els.connectLabel.textContent = connected ? 'Receiver connected' : 'Connect receiver';
  els.disconnectButton.hidden = !connected;
}

function urlBase64ToUint8Array(value) {
  const padding = '='.repeat((4 - value.length % 4) % 4);
  const base64 = (value + padding).replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(base64), character => character.charCodeAt(0));
}

async function responseJson(response) {
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error || `Server returned ${response.status}`);
  return body;
}

async function currentRegistration() {
  if (!('serviceWorker' in navigator)) throw new Error('Service workers are not supported on this device.');
  return navigator.serviceWorker.register('/notify/service-worker.js', { scope: '/notify/' });
}

async function refreshConnectionState() {
  if (!('serviceWorker' in navigator) || !('PushManager' in window)) {
    setConnected(false);
    setFeedback('This browser does not support Web Push. On iPhone, use iOS 16.4 or newer.', 'error');
    els.connectButton.disabled = true;
    return;
  }
  const registration = await currentRegistration();
  const subscription = await registration.pushManager.getSubscription();
  setConnected(Boolean(subscription && Notification.permission === 'granted'));
}

async function connect(event) {
  event.preventDefault();
  if (!isStandalone && /iPhone|iPad|iPod/.test(navigator.userAgent)) {
    els.installNote.hidden = false;
    setFeedback('Install Relay on your home screen, then open it there to enable notifications.', 'error');
    return;
  }

  els.connectButton.disabled = true;
  setFeedback('Tuning the receiver…');
  try {
    const apiUrl = els.apiUrl.value.trim().replace(/\/$/, '');
    const apiKey = els.apiKey.value.trim();
    if (!apiUrl.startsWith('https://') && !apiUrl.startsWith('http://localhost')) {
      throw new Error('The API must use HTTPS.');
    }

    const registration = await currentRegistration();
    const keyResponse = await fetch(endpoint('/vapid-public-key'), {
      headers: authHeaders(apiKey),
    }).then(responseJson);

    const permission = await Notification.requestPermission();
    if (permission !== 'granted') throw new Error('Notification permission was not granted.');

    let subscription = await registration.pushManager.getSubscription();
    if (!subscription) {
      subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: urlBase64ToUint8Array(keyResponse.public_key),
      });
    }

    const payload = subscription.toJSON();
    payload.deviceName = navigator.userAgent.includes('iPhone') ? 'iPhone · Stardive Relay' : 'Stardive Relay';
    const saved = await fetch(endpoint('/subscriptions'), {
      method: 'POST',
      headers: authHeaders(apiKey, true),
      body: JSON.stringify(payload),
    }).then(responseJson);

    localStorage.setItem(STORAGE.apiUrl, apiUrl);
    localStorage.setItem(STORAGE.apiKey, apiKey);
    localStorage.setItem(STORAGE.subscriptionId, saved.subscription.id);
    setConnected(true);
    setFeedback('Receiver tuned. You can close Relay now.', 'success');
  } catch (error) {
    setConnected(false);
    setFeedback(error.message || 'Could not connect this receiver.', 'error');
  } finally {
    els.connectButton.disabled = false;
  }
}

async function disconnect() {
  els.disconnectButton.disabled = true;
  setFeedback('Disconnecting…');
  try {
    const registration = await currentRegistration();
    const subscription = await registration.pushManager.getSubscription();
    const id = localStorage.getItem(STORAGE.subscriptionId);
    const apiKey = els.apiKey.value.trim();
    if (id) {
      const response = await fetch(endpoint(`/subscriptions/${encodeURIComponent(id)}`), {
        method: 'DELETE',
        headers: authHeaders(apiKey),
      });
      if (!response.ok && response.status !== 404) await responseJson(response);
    }
    if (subscription) await subscription.unsubscribe();
    localStorage.removeItem(STORAGE.subscriptionId);
    setConnected(false);
    setFeedback('This device is no longer receiving signals.');
  } catch (error) {
    setFeedback(error.message || 'Could not disconnect this receiver.', 'error');
  } finally {
    els.disconnectButton.disabled = false;
  }
}

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('stardive-relay', 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('signals')) {
        database.createObjectStore('signals', { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function readSignals() {
  const database = await openDatabase();
  return new Promise((resolve, reject) => {
    const request = database.transaction('signals').objectStore('signals').getAll();
    request.onsuccess = () => resolve(request.result.sort((a, b) => new Date(b.received_at) - new Date(a.received_at)).slice(0, 30));
    request.onerror = () => reject(request.error);
  });
}

async function clearSignals() {
  const database = await openDatabase();
  await new Promise((resolve, reject) => {
    const request = database.transaction('signals', 'readwrite').objectStore('signals').clear();
    request.onsuccess = resolve;
    request.onerror = () => reject(request.error);
  });
  renderSignals([]);
}

function escapeHtml(value = '') {
  return value.replace(/[&<>'"]/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' })[character]);
}

function renderSignals(signals) {
  els.emptyLog.hidden = signals.length > 0;
  els.signalList.innerHTML = signals.map(signal => {
    const received = new Date(signal.received_at || signal.sent_at || Date.now());
    return `<li class="signal">
      <div class="signal-meta"><span>${escapeHtml(signal.channel || 'STARDIVE')}</span><time>${received.toLocaleString([], { dateStyle: 'short', timeStyle: 'short' })}</time></div>
      <h3>${escapeHtml(signal.title || 'New signal')}</h3>
      <p>${escapeHtml(signal.body || '')}</p>
    </li>`;
  }).join('');
}

els.form.addEventListener('submit', connect);
els.disconnectButton.addEventListener('click', disconnect);
els.clearLog.addEventListener('click', clearSignals);
els.revealKey.addEventListener('click', () => {
  const showing = els.apiKey.type === 'text';
  els.apiKey.type = showing ? 'password' : 'text';
  els.revealKey.textContent = showing ? 'SHOW' : 'HIDE';
});
navigator.serviceWorker?.addEventListener('message', event => {
  if (event.data?.type === 'signal-received') readSignals().then(renderSignals);
});

Promise.all([refreshConnectionState(), readSignals().then(renderSignals)]).catch(error => {
  setFeedback(error.message, 'error');
});

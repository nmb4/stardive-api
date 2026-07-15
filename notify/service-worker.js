const CACHE = 'stardive-relay-v1';
const SHELL = [
  '/notify/',
  '/notify/styles.css',
  '/notify/app.js',
  '/notify/manifest.webmanifest',
  '/notify/icons/icon-192.png',
  '/notify/icons/icon-512.png',
];

self.addEventListener('install', event => {
  event.waitUntil(caches.open(CACHE).then(cache => cache.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener('activate', event => {
  event.waitUntil(
    caches.keys()
      .then(keys => Promise.all(keys.filter(key => key !== CACHE).map(key => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', event => {
  if (event.request.method !== 'GET' || new URL(event.request.url).origin !== self.location.origin) return;
  event.respondWith(fetch(event.request).catch(() => caches.match(event.request).then(response => response || caches.match('/notify/'))));
});

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('stardive-relay', 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('signals')) database.createObjectStore('signals', { keyPath: 'id' });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function storeSignal(signal) {
  const database = await openDatabase();
  const record = { ...signal, id: signal.id || crypto.randomUUID(), received_at: new Date().toISOString() };
  await new Promise((resolve, reject) => {
    const request = database.transaction('signals', 'readwrite').objectStore('signals').put(record);
    request.onsuccess = resolve;
    request.onerror = () => reject(request.error);
  });
  const records = await new Promise((resolve, reject) => {
    const request = database.transaction('signals').objectStore('signals').getAll();
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  if (records.length > 50) {
    records.sort((a, b) => new Date(a.received_at) - new Date(b.received_at));
    const transaction = database.transaction('signals', 'readwrite');
    records.slice(0, records.length - 50).forEach(item => transaction.objectStore('signals').delete(item.id));
  }
  return record;
}

self.addEventListener('push', event => {
  let signal = { title: 'New Stardive signal', body: 'Open Relay to view it.' };
  try {
    if (event.data) signal = { ...signal, ...event.data.json() };
  } catch {
    signal.body = event.data?.text() || signal.body;
  }
  event.waitUntil((async () => {
    const record = await storeSignal(signal);
    await self.registration.showNotification(record.title, {
      body: record.body,
      icon: record.icon || '/notify/icons/icon-192.png',
      badge: '/notify/icons/icon-192.png',
      tag: record.tag || record.id,
      data: { url: record.url || '/notify/', id: record.id },
    });
    const clients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
    clients.forEach(client => client.postMessage({ type: 'signal-received', signal: record }));
  })());
});

self.addEventListener('notificationclick', event => {
  event.notification.close();
  const target = event.notification.data?.url || '/notify/';
  event.waitUntil((async () => {
    const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
    const relay = windows.find(client => new URL(client.url).pathname.startsWith('/notify/'));
    if (target.startsWith('/') && relay) {
      await relay.focus();
      relay.navigate(target);
      return;
    }
    await self.clients.openWindow(target);
  })());
});

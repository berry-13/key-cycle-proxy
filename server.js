const http = require('http');
const https = require('https');
const config = require('./config.json');

const apiKeys = config.apiKeys;
const PORT = 3456;

let currentApiKeyIndex = 0;
let attemptCount = 0;

updateLatencies().catch((err) => console.error('Failed to update latencies', err));
setInterval(updateLatencies, 60000);

async function measureLatency(apiKeyInfo) {
  return new Promise((resolve) => {
    try {
      const url = new URL(apiKeyInfo.url);
      const transport = url.protocol === 'https:' ? https : http;
      const start = Date.now();
      const req = transport.request(url, { method: 'HEAD' }, (res) => {
        res.on('end', () => resolve(Date.now() - start));
        res.resume();
      });
      req.on('error', () => resolve(Number.MAX_SAFE_INTEGER));
      req.setTimeout(5000, () => {
        req.destroy();
        resolve(Number.MAX_SAFE_INTEGER);
      });
      req.end();
    } catch {
      resolve(Number.MAX_SAFE_INTEGER);
    }
  });
}

async function updateLatencies() {
  const latencies = await Promise.all(apiKeys.map((k) => measureLatency(k)));
  latencies.forEach((latency, i) => {
    apiKeys[i].latency = latency;
  });
  apiKeys.sort((a, b) => a.latency - b.latency);
  currentApiKeyIndex = 0;
  console.log('Updated proxy latencies:', apiKeys.map(k => `${k.url}:${k.latency}`).join(', '));
}

function getNextApiKey() {
  const numKeys = apiKeys.length;
  currentApiKeyIndex = (currentApiKeyIndex + 1) % numKeys;
  return apiKeys[currentApiKeyIndex];
}

function getBestApiKeyForModel(model) {
  for (let i = 0; i < apiKeys.length; i++) {
    const info = apiKeys[i];
    if (info.models.includes(model) || info.models.includes('others')) {
      currentApiKeyIndex = i;
      return info;
    }
  }
  return undefined;
}

function forwardToOpenAI(apiKeyInfo, url, data, res) {
  const { key, url: apiUrl } = apiKeyInfo;
  const openaiUrl = apiUrl + url;
  
  const parsedUrl = new URL(openaiUrl);

  const transport = parsedUrl.protocol === 'https:' ? https : http;

  const options = {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${encodeURIComponent(key)}`,
    },
  };

  const req = transport.request(parsedUrl, options, (proxyRes) => {
    console.log('Received response from the reverse proxy. Status:', proxyRes.statusCode);

    if (proxyRes.statusCode === 429 || proxyRes.statusCode === 418 || proxyRes.statusCode === 502 || proxyRes.statusCode === 400) {
      handleReverseProxyError(res, url, data);
    } else {
      res.writeHead(proxyRes.statusCode, proxyRes.headers);
      proxyRes.pipe(res);
    }
  });

  req.on('error', (error) => {
    console.error('Error sending request to OpenAI:', error);
    handleReverseProxyError(res, url, data);
  });

  getNextApiKey();
  req.write(data);
  req.end();
}


function checkModel(apiKey, model, url, data, res) {
  if (apiKey === undefined) {
    res.statusCode = 500;
    res.end(JSON.stringify({ error: 'No API key found' }));
    return;
  }

  const apiKeyInfo = apiKeys.find((info) => info.key === apiKey);

  if (!apiKeyInfo) {
    res.statusCode = 500;
    res.end(JSON.stringify({ error: 'Invalid API key' }));
  } else if (apiKeyInfo.models.includes(model) || apiKeyInfo.models.includes('others')) {
    attemptCount = 0;
    console.log(`Forwarding to ${apiKeyInfo.url} with API key: ${apiKey}`);
    forwardToOpenAI(apiKeyInfo, url, data, res);
  } else {
    console.log('Model not supported by this API key');
    modelNotSupported(apiKey, model, url, data, res);
  }
}

function handleReverseProxyError(res, url, data) {
  console.log('Error from the reverse proxy. Changing API key and retrying.');
  const newApiKeyInfo = getNextApiKey();
  forwardToOpenAI(newApiKeyInfo, url, data, res);
  console.log('Forwarding to', newApiKeyInfo.url, 'with API key:', newApiKeyInfo.key);
}

async function modelNotSupported(apiKey, model, url, data, res) {
  if (attemptCount >= apiKeys.length) {
    // All API keys have been tried and none support the model
    res.statusCode = 500;
    res.end(JSON.stringify({ error: 'No API key available for this model' }));
    return;
  }

  attemptCount++; // Increment the count of attempts
  const newApiKeyInfo = getNextApiKey();
  checkModel(newApiKeyInfo.key, model, url, data, res);
}

http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  res.setHeader('Access-Control-Allow-Origin', '*');
  console.log('Received POST request:', req.url);

  if (req.method !== 'POST') {
    res.statusCode = 405; // Method Not Allowed
    res.end();
    return;
  }

  let data = '';
  req.on('data', (chunk) => {
    data += chunk;
  });

  req.on('end', () => {
    try {
      const payload = JSON.parse(data);
      const model = payload.model;

      const apiKeyInfo = getBestApiKeyForModel(model);
      const apiKey = apiKeyInfo ? apiKeyInfo.key : undefined;
      checkModel(apiKey, model, req.url, data, res);
    } catch (error) {
      console.error('Error processing request:', error);
      res.statusCode = 400; // Bad Request
      res.end(JSON.stringify({ error: 'Invalid JSON payload' }));
    }
  });
}).listen(PORT, 'localhost', () => {
  console.log(`Server running at http://localhost:${PORT}/`);
});

const http = require('http');
const https = require('https');
const config = require('./config.json');

const apiKeys = config.apiKeys;

let currentApiKeyIndex = 0;
let attemptCount = 0;

function getNextApiKey() {
  const numKeys = apiKeys.length;
  currentApiKeyIndex = (currentApiKeyIndex + 1) % numKeys;
  return apiKeys[currentApiKeyIndex];
}

function forwardToOpenAI(apiKeyInfo, url, data, res) {
  const { key, url: apiUrl } = apiKeyInfo;
  const openaiUrl = apiUrl + url;
  const options = {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${encodeURIComponent(key)}`,
    },
  };

  const req = https.request(openaiUrl, options, (proxyRes) => {
    console.log('Received response from the reverse proxy. Status:', proxyRes.statusCode);

    if (proxyRes.statusCode === 429 || proxyRes.statusCode === 418 || proxyRes.statusCode === 502) {
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
  } else if (apiKeyInfo.models.includes(model)) {
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

      const apiKeyInfo = apiKeys[currentApiKeyIndex];
      const apiKey = apiKeyInfo.key;
      checkModel(apiKey, model, req.url, data, res);
    } catch (error) {
      console.error('Error processing request:', error);
      res.statusCode = 400; // Bad Request
      res.end(JSON.stringify({ error: 'Invalid JSON payload' }));
    }
  });
}).listen(3456, '192.168.1.34', () => {
  console.log('Server running at http://192.168.1.34:3456/');
});

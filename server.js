const http = require('http');
const https = require('https');
const { URL } = require('url');
const config = require('./config.json');

const { apiKeys } = config;
let currentApiKeyIndex = 0;

function getNextApiKey() {
  currentApiKeyIndex = (currentApiKeyIndex + 1) % apiKeys.length;
  return apiKeys[currentApiKeyIndex];
}

function forwardToOpenAI(apiKeyInfo, url, data, res) {
  const { key, url: apiUrl } = apiKeyInfo;
  const openaiUrl = new URL(url, apiUrl);
  const transport = openaiUrl.protocol === 'https:' ? https : http;

  const options = {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${key}`,
    },
  };

  const req = transport.request(openaiUrl, options, (proxyRes) => {
    console.log('Received response from the reverse proxy. Status:', proxyRes.statusCode);
    
    if ([400, 418, 429, 502].includes(proxyRes.statusCode)) {
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

  req.write(data);
  req.end();
}

function checkModel(apiKey, model, url, data, res, attemptCount = 0) {
  if (attemptCount >= apiKeys.length) {
    res.writeHead(500, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'No API key available for this model' }));
    return;
  }

  const apiKeyInfo = apiKeys.find((info) => info.key === apiKey);

  if (!apiKeyInfo) {
    res.writeHead(500, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'Invalid API key' }));
  } else if (apiKeyInfo.models.includes(model)) {
    console.log(`Forwarding to ${apiKeyInfo.url} with API key: ${apiKey}`);
    forwardToOpenAI(apiKeyInfo, url, data, res);
  } else {
    console.log('Model not supported by this API key');
    const newApiKeyInfo = getNextApiKey();
    checkModel(newApiKeyInfo.key, model, url, data, res, attemptCount + 1);
  }
}

function handleReverseProxyError(res, url, data) {
  console.log('Error from the reverse proxy. Changing API key and retrying.');
  const newApiKeyInfo = getNextApiKey();
  console.log('Forwarding to', newApiKeyInfo.url, 'with API key:', newApiKeyInfo.key);
  forwardToOpenAI(newApiKeyInfo, url, data, res);
}

http.createServer((req, res) => {
  if (req.method !== 'POST') {
    res.writeHead(405, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'Method Not Allowed' }));
    return;
  }

  console.log('Received POST request:', req.url);
  
  let data = '';
  req.on('data', (chunk) => { data += chunk; });
  req.on('end', () => {
    try {
      const { model } = JSON.parse(data);
      const apiKeyInfo = apiKeys[currentApiKeyIndex];
      checkModel(apiKeyInfo.key, model, req.url, data, res);
    } catch (error) {
      console.error('Error processing request:', error);
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'Invalid JSON payload' }));
    }
  });
}).listen(3456, 'localhost', () => {
  console.log('Server running at http://localhost:3456/');
});

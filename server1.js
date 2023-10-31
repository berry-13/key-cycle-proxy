const http = require('http');
const https = require('https');
const fs = require('fs');
const config = require('./config.json');

const apiKeysNaga = config.apiKeys.naga;
const apiKeysNova = config.apiKeys.nova;
const validApiKey = config.validApiKey;

let currentApiKeyNagaIndex = 0;
let currentApiKeyNovaIndex = 0;

function getNextApiKeyNaga() {
  let index = 0;
  const numKeysNaga = apiKeysNaga.length;
  return function() {
    const apiKeyNagaInfo = apiKeysNaga[index];
    index = (index + 1) % numKeysNaga;
    return { apiKeyNaga: apiKeyNagaInfo.key, apiUrlNaga: apiKeyNagaInfo.url };
  };
}

function getNextApiKeyNova() {
  let index = 0;
  const numKeysNova = apiKeysNova.length;
  return function() {
    const apiKeyNovaInfo = apiKeysNova[index];
    index = (index + 1) % numKeysNova;
    return { apiKeyNova: apiKeyNovaInfo.key, apiUrlNova: apiKeyNovaInfo.url };
  };
}

function forwardToOpenAI(apiUrl, url, data, apiKey, res, models) {
    const openaiUrl = apiUrl + url;
    const options = {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': 'Bearer ' + encodeURIComponent(apiKey)
      }
    };
    const req = https.request(openaiUrl, options, (proxyRes) => {
      console.log('Received response from the reverse proxy. Status:', proxyRes.statusCode);
  
      res.writeHead(proxyRes.statusCode, proxyRes.headers);
      proxyRes.pipe(res);
    });
  
    req.on('error', (error) => {
      console.error('Error sending request to OpenAI:', error);
      res.statusCode = 500;
      res.end();
    });
  
    req.write(data);
    req.end();
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
  
    const chunks = [];
    req.on('data', (chunk) => {
      chunks.push(chunk);
    });
  
    req.on('end', () => {
      const data = Buffer.concat(chunks).toString();
      const url = req.url.split('?')[0];
  
      try {
        const authorizationHeader = req.headers['authorization'];
        const providedApiKey = authorizationHeader ? authorizationHeader.split(' ')[1] : null;
        const payload = JSON.parse(data);
        const model = payload.model;
  
        console.log('Received API key:', providedApiKey);
  
        if (!providedApiKey || providedApiKey !== validApiKey) {
          console.log('Invalid API key');
          res.statusCode = 401;
          res.end(JSON.stringify({ error: 'Invalid API key' }));
          return;
        } else {
          const apiKeyInfo = config.apiKeys.find(apiKeyInfo => apiKeyInfo.key === providedApiKey);
  
          if (apiKeyInfo) {
            const apiKey = apiKeyInfo.key;
            const apiUrl = apiKeyInfo.url;
            const models = apiKeyInfo.models;
  
            if (models.includes(model)) {
              console.log(`Forwarding to ${apiUrl} with API key: ${apiKey}`);
              forwardToOpenAI(apiUrl, url, data, apiKey, res, models);
            } else {
              console.log('Model not supported by this API key');
              res.statusCode = 400;
              res.end(JSON.stringify({ error: 'Model not supported by this API key' }));
            }
          } else {
            console.log('Invalid API key');
            res.statusCode = 401;
            res.end(JSON.stringify({ error: 'Invalid API key' }));
          }
        }
      } catch (error) {
        console.error('Error processing request:', error);
        res.statusCode = 400; // Bad Request
        res.end(JSON.stringify({ error: 'Invalid JSON payload' }));
      }
    });
  }).listen(3456, '192.168.1.34', () => {
    console.log('Server running at http://192.168.1.34:3456/');
  });

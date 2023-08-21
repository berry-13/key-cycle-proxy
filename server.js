const http = require('http');
const https = require('https');
const fs = require('fs');

const config = JSON.parse(fs.readFileSync('config.json', 'utf8'));

// Funzione per ottenere la prossima chiave API nella rotazione
function getNextApiKey() {
  let index = 0;
  const numKeys = config.apiKeys.length;
  return function() {
    const apiKey = config.apiKeys[index];
    index = (index + 1) % numKeys;
    return apiKey;
  };
}

function forwardToOpenAI(url, data, apiKey, res) {
  const openaiUrl = 'https://chimeragpt.adventblocks.cc/api' + url;

  const options = {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': 'Bearer ' + apiKey
    }
  };

  const req = https.request(openaiUrl, options, (proxyRes) => {
    res.writeHead(proxyRes.statusCode, proxyRes.headers); // Pass through response status and headers
    proxyRes.pipe(res); // Pipe the proxy response directly to the client response
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
  res.setHeader('Content-Type', 'application/json'); // Set appropriate Content-Type
  res.setHeader('Access-Control-Allow-Origin', '*');

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
      const payload = JSON.parse(data);
      const providedApiKey = payload.apiKey;

      if (!providedApiKey || providedApiKey !== config.validApiKey) {
        res.statusCode = 401;
        res.end(JSON.stringify({ error: 'Invalid API key' }));
        return;
      }

      if (url && url.includes('/v1/')) {
        forwardToOpenAI(url, data, getNextApiKey(), res);
      } else {
        res.statusCode = 404;
        res.end(JSON.stringify({ error: 'Not found' }));
      }
    } catch (error) {
      res.statusCode = 400; // Bad Request
      res.end(JSON.stringify({ error: 'Invalid JSON payload' }));
    }
  });
}).listen(3000, 'localhost', () => {
  console.log('Server running at http://localhost:3000/');
});

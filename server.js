const http = require("http");
const https = require("https");
const config = require("./config.json");

const apiKeys = config.apiKeys;

let currentApiKeyIndex = 0;

function getNextApiKey() {
  const numKeys = apiKeys.length;
  currentApiKeyIndex = (currentApiKeyIndex + 1) % numKeys;
  const apiKeyInfo = apiKeys[currentApiKeyIndex];
  return {
    apiKey: apiKeyInfo.key,
    apiUrl: apiKeyInfo.url,
    models: apiKeyInfo.models,
  };
}

function forwardToOpenAI(apiUrl, url, data, apiKey, res, models) {
  const openaiUrl = apiUrl + url;
  const options = {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: "Bearer " + encodeURIComponent(apiKey),
    },
  };
  const req = https.request(openaiUrl, options, (proxyRes) => {
    console.log(
      "Received response from the reverse proxy. Status:",
      proxyRes.statusCode
    );

    res.writeHead(proxyRes.statusCode, proxyRes.headers);
    proxyRes.pipe(res);
  });

  req.on("error", (error) => {
    console.error("Error sending request to OpenAI:", error);
    res.statusCode = 500;
    res.end();

    // Handle the error by changing the API key and retrying
    handleReverseProxyError(apiUrl, url, data, models, res);
  });

  req.write(data);
  req.end();
}


function checkModel(apiKey, apiUrl, models, model, url, data, res) {
  const apiKeyInfo = config.apiKeys.find(info => info.key === apiKey);

  if (!apiKeyInfo) {
    res.statusCode = 500;
    res.end(JSON.stringify({ error: "Invalid API key" }));
    return;
  }

  if (models.includes(model)) {
    console.log(`Forwarding to ${apiUrl} with API key: ${apiKey}`);
    forwardToOpenAI(apiUrl, url, data, apiKey, res, models);
  } else {
    console.log("Model not supported by this API key");
    res.statusCode = 400;
    res.end(JSON.stringify({ error: "Model not supported by this API key" }));
  }

  // Aggiorna l'indice della chiave API in base all'API utilizzata
  const newApiKeyInfo = getNextApiKey();
  currentApiKeyIndex = config.apiKeys.findIndex(info => info.key === newApiKeyInfo.apiKey);
}

function handleApiKeyRotationAndRetry(apiUrl, url, data, apiKey, res, models) {
  forwardToOpenAI(apiUrl, url, data, apiKey, res, models);
}

function handleReverseProxyError(apiUrl, url, data, models, res) {
  console.log("Error from reverse proxy. Changing API key and retrying.");
  const newApiKeyInfo = getNextApiKey();
  handleApiKeyRotationAndRetry(
    newApiKeyInfo.apiUrl,
    url,
    data,
    newApiKeyInfo.apiKey,
    res,
    models
  );
}

http.createServer((req, res) => {
  res.setHeader("Content-Type", "application/json");
  res.setHeader("Access-Control-Allow-Origin", "*");
  console.log("Received POST request:", req.url);

  if (req.method !== "POST") {
    res.statusCode = 405; // Method Not Allowed
    res.end();
    return;
  }

  const chunks = [];
  req.on("data", (chunk) => {
    chunks.push(chunk);
  });

  req.on("end", () => {
    const data = Buffer.concat(chunks).toString();
    const url = req.url.split("?")[0];

    try {
      const payload = JSON.parse(data);
      const model = payload.model;

      const apiKeyInfo = config.apiKeys[currentApiKeyIndex];
      const apiKey = apiKeyInfo.key;
      const apiUrl = apiKeyInfo.url;
      const models = apiKeyInfo.models;
      console.log(url, apiUrl);

      forwardToOpenAI(apiUrl, url, data, apiKey, res, models);
    } catch (error) {
      console.error("Error processing request:", error);
      res.statusCode = 400; // Bad Request
      res.end(JSON.stringify({ error: "Invalid JSON payload" }));
    }
  });
}).listen(3456, "localhost", () => {
  console.log("Server running at http://localhost:3456/");
});

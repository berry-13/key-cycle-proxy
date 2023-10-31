const http = require("http");
const https = require("https");
const config = require("./config.json");

const apiKeys = config.apiKeys;
const validApiKey = "j176p6rf1qyy7vc7wxp7j4cw"
// const validApiKey = config.validApiKey;

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
  });

  req.write(data);
  req.end();
}

function checkModel(apiKey, apiUrl, models, model, url, data, res) {
  if (models.includes(model)) {
    console.log(`Forwarding to ${apiUrl} with API key: ${apiKey}`);
    forwardToOpenAI(apiUrl, url, data, apiKey, res, models);
    getNextApiKey();
  } else {
    getNextApiKey();
    console.log("Model not supported by this API key");
    res.statusCode = 400;
    res.end(JSON.stringify({ error: "Model not supported by this API key" }));
  }
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
      const authorizationHeader = req.headers["authorization"];
      const providedApiKey = authorizationHeader
      ? authorizationHeader.split(" ")[1].trim()
      : null;  
      const payload = JSON.parse(data);
      const model = payload.model;

      console.log("Received API key:", providedApiKey);
      console.log("Received model:", model);
      console.log("validApiKey:", validApiKey);

      if (!providedApiKey || providedApiKey.trim() !== validApiKey.trim()) {
        console.log('Invalid API key');
        res.statusCode = 401;
        res.end(JSON.stringify({ error: 'Invalid API key' }));
        return;
      } else {
        const apiKeyInfo = config.apiKeys.find(
          (apiKeyInfo) => apiKeyInfo.key === providedApiKey
        );

        if (apiKeyInfo) {
          const apiKey = apiKeyInfo.key;
          const apiUrl = apiKeyInfo.url;
          const models = apiKeyInfo.models;

          checkModel(apiKey, apiUrl, models, model, url, data, res);
        } else {
          console.log("Invalid API key");
          res.statusCode = 401;
          res.end(JSON.stringify({ error: "Invalid API key" }));
        }
      }
    } catch (error) {
      console.error("Error processing request:", error);
      res.statusCode = 400; // Bad Request
      res.end(JSON.stringify({ error: "Invalid JSON payload" }));
    }
  });
}).listen(3456, "localhost", () => {
  console.log("Server running at http://localhost:3456/");
});

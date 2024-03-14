<p align="center">
    <img src="https://github.com/Berry-13/API-Key-Rotator/assets/81851188/3a17e214-ff55-418d-bdac-524a1c553503" height="256">
    <h1 align="center">KeyCycleProxy</h1>
</p>

The KeyCycleProxy is a Node.js-based server that serves as a reverse proxy for reverse proxies and OpenAI. This server allows you to efficiently route requests to reverse proxies or OpenAI while automatically rotating between multiple API keys to ensure uninterrupted service

## Features

- Automatic rotation of API keys to prevent rate limiting
- Efficient routing of requests to supported models
- Custom API base URL (you can use it as a reverse proxy for reverse proxies)

## Prerequisites

Before you begin, ensure you have met the following requirements:

- Node.js installed on your machine
- API keys and configurations in the `config.json` file

## Installation

1. Clone this repository to your local machine
2. Install the required dependencies by running:
 `npm install`
3. Configure your API keys in the config.json file

```json
  {
    "apiKeys": [
      {
        "key": "api-key-1",
        "url": "api-base-url-",
        "models": ["model"]
      }
    ]
  }
  

```


- `api-key-1` is the API key from OpenAI or your reverse proxy

- `api-base-url` is the URL that the server needs to use for making requests. For example, for OpenAI, it is `https://api.openai.com`

- `model` represents the model that the API is configured to accept.

For instance, if you set `gpt-3.5-turbo` as model 1, this API key will be used exclusively for this model. 
Then, for example, if you set model 2 as `gpt-4, others` this API key will be used for GPT-4 and all other models not explicitly specified in the config.json, such as gpt-4-32k, gpt-3.5-turbo-0301, gpt-3.5-turbo-16k, etc. 
This excludes gpt-3.5-turbo, as it was specified under model 1.

Here's an example of how it could be configured:

```
  {
    "apiKeys": [
      {
        "key": "sk-4dy7adya89dyh3sca68a78yauwsdhjf",
        "url": "https://api.openai.com",
        "models": [gpt-3-5-turbo, gpt-3.5-turbo-0301, gpt-3.5-turbo-16k]
      },
      {
        "key": "tr-dwadaw78xcawoidja0'w9dia9dwf",
        "url": "https://example.com",
        "models": [gpt-4, gpt-4-32k, gpt-4-0314]
      },
      {
        "key": "skdwdw90d89wud09aduajwiodkwd893",
        "url": "https://api.another.com",
        "models": [others]
      }
    ]
  }
```


## Usage
Start the server by running the command `node server.js`

The server will automatically handle the routing and key rotation for you

## Planned
- [ ] choose the best latency proxy

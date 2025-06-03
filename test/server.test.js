const path = require('path');
const fs = require('fs');
const http = require('http');
const { spawn } = require('child_process');
const request = require('supertest');

describe('server', function () {
  let proxyServer;
  let serverProcess;
  before(function (done) {
    // fake upstream server that always returns 200
    proxyServer = http.createServer((req, res) => {
      res.statusCode = 200;
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({ ok: true }));
    }).listen(5678, 'localhost', () => {
      const config = {
        apiKeys: [
          { key: 'test-key', url: 'http://localhost:5678', models: ['test-model'] }
        ]
      };
      fs.writeFileSync(path.join(__dirname, '..', 'config.json'), JSON.stringify(config));
      serverProcess = spawn('node', ['server.js'], {
        cwd: path.join(__dirname, '..'),
        stdio: ['ignore', 'pipe', 'inherit']
      });
      serverProcess.stdout.on('data', (data) => {
        if (data.toString().includes('Server running')) {
          done();
        }
      });
    });
  });

  after(function (done) {
    serverProcess.kill();
    fs.unlinkSync(path.join(__dirname, '..', 'config.json'));
    proxyServer.close(done);
  });

  it('responds with 200 for valid api key', function (done) {
    request('http://localhost:3456')
      .post('/v1/test')
      .send({ model: 'test-model' })
      .expect(200, done);
  });
});

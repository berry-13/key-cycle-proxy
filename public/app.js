const e = React.createElement;
const { useState, useEffect } = React;

function App() {
  const [config, setConfig] = useState({ apiKeys: [] });
  const [newKey, setNewKey] = useState({ key: '', url: '', models: '' });

  useEffect(() => {
    fetch('/api/config')
      .then((r) => r.json())
      .then((data) => setConfig(data));
  }, []);

  function save(updated) {
    fetch('/api/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(updated),
    });
  }

  function addKey() {
    const models = newKey.models.split(',').map((m) => m.trim()).filter(Boolean);
    const updated = { apiKeys: [...config.apiKeys, { key: newKey.key, url: newKey.url, models }] };
    setConfig(updated);
    save(updated);
    setNewKey({ key: '', url: '', models: '' });
  }

  function removeKey(index) {
    const updated = { apiKeys: config.apiKeys.filter((_, i) => i !== index) };
    setConfig(updated);
    save(updated);
  }

  return (
    e('div', { className: 'max-w-xl mx-auto space-y-4' },
      e('h1', { className: 'text-2xl font-bold' }, 'Config'),
      config.apiKeys.map((k, i) => (
        e('div', { key: i, className: 'border p-2 rounded' },
          e('div', { className: 'font-mono break-all text-sm' }, k.key),
          e('div', { className: 'text-sm' }, k.url),
          e('div', { className: 'text-sm' }, k.models.join(', ')),
          e('button', {
            className: 'mt-2 bg-red-500 text-white px-2 py-1 rounded',
            onClick: () => removeKey(i)
          }, 'Remove')
        )
      )),
      e('div', { className: 'border p-2 rounded space-y-2' },
        e('input', {
          className: 'w-full border p-1 rounded',
          placeholder: 'API Key',
          value: newKey.key,
          onChange: (ev) => setNewKey({ ...newKey, key: ev.target.value })
        }),
        e('input', {
          className: 'w-full border p-1 rounded',
          placeholder: 'URL',
          value: newKey.url,
          onChange: (ev) => setNewKey({ ...newKey, url: ev.target.value })
        }),
        e('input', {
          className: 'w-full border p-1 rounded',
          placeholder: 'Models (comma separated)',
          value: newKey.models,
          onChange: (ev) => setNewKey({ ...newKey, models: ev.target.value })
        }),
        e('button', {
          className: 'bg-blue-500 text-white px-2 py-1 rounded',
          onClick: addKey
        }, 'Add')
      )
    )
  );
}

ReactDOM.render(e(App), document.getElementById('root'));

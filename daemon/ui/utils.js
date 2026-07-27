    let descriptors = [];
    let discoverableDescriptors = [];
    let byId = {};
    const THEME_OPTIONS = [
      { id: 'light', label: 'Light' },
      { id: 'dark', label: 'Dark' },
    ];
    const THEMES = {
      light: {
        bg: '#ffffff', panel: '#f6f6f6', panel2: '#f7f7f7', panel3: '#ffffff',
        line: '#dddddd', text: '#1f1f1f', muted: '#6f6f6f',
        accent: '#ffb000', accentSoft: 'rgba(255,176,0,.18)', accentText: '#1f1f1f'
      },
      dark: {
        bg: '#1e1e1e', panel: '#262626', panel2: '#242424', panel3: '#1f1f1f',
        line: '#3a3a3a', text: '#dcddde', muted: '#a6a6a6',
        accent: '#ffb000', accentSoft: 'rgba(255,176,0,.2)', accentText: '#1f1f1f'
      },
    };
    let nextSettingsRequestId = 0;
    const pendingSettingsRequests = new Map();

    window.addEventListener('bones-message', event => {
      let response = event.detail;
      if (typeof response === 'string') {
        try {
          response = JSON.parse(response);
        } catch (_) {
          return;
        }
      }
      if (!response || response.protocol !== model.settingsProtocol) return;
      const pending = pendingSettingsRequests.get(response.requestId);
      if (!pending) return;
      pendingSettingsRequests.delete(response.requestId);
      pending(response);
    });

    function copperFetch(path, options = {}) {
      if (model.transport !== 'bones') {
        return fetch(path, options);
      }
      const requestId = `settings-${++nextSettingsRequestId}`;
      let body;
      if (options.body) {
        body = typeof options.body === 'string' ? JSON.parse(options.body) : options.body;
      }
      const request = {
        protocol: model.settingsProtocol,
        requestId,
        method: options.method || 'GET',
        path,
        body
      };
      return new Promise((resolve, reject) => {
        pendingSettingsRequests.set(requestId, response => {
          resolve({
            ok: response.ok,
            status: response.status,
            json: async () => response.data,
            text: async () => response.error || JSON.stringify(response.data || {})
          });
        });
        try {
          window.ipc.postMessage(JSON.stringify(request));
        } catch (error) {
          pendingSettingsRequests.delete(requestId);
          reject(error);
        }
      });
    }

    function normalizeThemeId(themeId) {
      const normalized = String(themeId || 'light').trim().toLowerCase();
      if (normalized === 'obsidian-light') return 'light';
      if (normalized === 'obsidian-dark') return 'dark';
      return Object.prototype.hasOwnProperty.call(THEMES, normalized) ? normalized : 'light';
    }

    function resolveTheme(themeId) {
      const normalized = normalizeThemeId(themeId);
      return THEMES[normalized] || THEMES.light;
    }

    function applyTheme(themeId) {
      const palette = resolveTheme(themeId);
      document.documentElement.style.setProperty('--bg', palette.bg);
      document.documentElement.style.setProperty('--panel', palette.panel);
      document.documentElement.style.setProperty('--panel2', palette.panel2);
      document.documentElement.style.setProperty('--panel3', palette.panel3);
      document.documentElement.style.setProperty('--line', palette.line);
      document.documentElement.style.setProperty('--text', palette.text);
      document.documentElement.style.setProperty('--muted', palette.muted);
      document.documentElement.style.setProperty('--accent', palette.accent);
      document.documentElement.style.setProperty('--accent-soft', palette.accentSoft);
      document.documentElement.style.setProperty('--accent-text', palette.accentText);
    }

    function replaceDescriptorModel(nextModel, syncSelection = true) {
      descriptors = Array.isArray(nextModel.descriptors) ? nextModel.descriptors : [];
      discoverableDescriptors = Array.isArray(nextModel.discoverableDescriptors)
        ? nextModel.discoverableDescriptors
        : descriptors;
      byId = Object.fromEntries(descriptors.map(d => [d.id, d]));
      model.coreExtensionIds = Array.isArray(nextModel.coreExtensionIds)
        ? nextModel.coreExtensionIds
        : (Array.isArray(model.coreExtensionIds) ? model.coreExtensionIds : []);
      model.selectedExtensionId = nextModel.selectedExtensionId || '';
      if (syncSelection && currentSection && currentSection.startsWith('ext:')) {
        const id = currentSection.slice(4);
        if (!byId[id]) {
          currentSection = 'core';
          currentTab = '';
          updateUrl();
        }
      }
    }

    replaceDescriptorModel(model, false);
    let currentSection = (() => {
      const defaultSection = model.selectedExtensionId ? `ext:${model.selectedExtensionId}` : 'core';
      const params = new URLSearchParams(window.location.search);
      const requested = params.get('section');
      if (!requested) {
        return defaultSection;
      }
      if (requested === 'core') {
        return 'core';
      }
      if (requested === 'commands') {
        return 'commands';
      }
      if (requested.startsWith('ext:')) {
        const id = requested.slice(4);
        if (byId[id]) {
          return requested;
        }
      }
      return defaultSection;
    })();
    let currentTab = (() => {
      const params = new URLSearchParams(window.location.search);
      return params.get('tab') || '';
    })();

    const navEl = document.getElementById('nav');
    const contentViewEl = document.getElementById('contentView');
    const statusMsgEl = document.getElementById('statusMsg');
    const closeBtn = document.getElementById('closeBtn');
    const pageEyebrowEl = document.getElementById('pageEyebrow');
    const pageTitleEl = document.getElementById('pageTitle');
    const pageSubEl = document.getElementById('pageSub');
    const tabsEl = document.getElementById('tabs');
    const saveBtn = document.getElementById('saveBtn');

    function currentDescriptor() {
      return currentSection.startsWith('ext:') ? byId[currentSection.slice(4)] : null;
    }

    if (!model.allowClose && closeBtn) {
      closeBtn.style.display = 'none';
    }

    function setStatus(msg) {
      statusMsgEl.textContent = msg;
    }

    function updateUrl() {
      const params = new URLSearchParams();
      params.set('section', currentSection);
      if (currentTab) {
        params.set('tab', currentTab);
      }
      window.history.replaceState(null, '', `${window.location.pathname}?${params.toString()}`);
    }

    function humanizeKey(value) {
      return String(value || '')
        .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
        .replace(/[-_]/g, ' ')
        .replace(/^./, ch => ch.toUpperCase());
    }

    function formatValue(value, format) {
      if (value === null || value === undefined || value === '') return 'Not set';
      if (format === 'date-time' && typeof value === 'number') {
        return new Date(value * 1000).toLocaleString();
      }
      if (typeof value === 'boolean') return value ? 'Enabled' : 'Disabled';
      if (typeof value === 'object') return JSON.stringify(value);
      return String(value);
    }

    function createCard(title, description) {
      const card = document.createElement('section');
      card.className = 'card';
      const heading = document.createElement('h2');
      heading.className = 'card-title';
      heading.textContent = title;
      card.appendChild(heading);
      if (description) {
        const desc = document.createElement('p');
        desc.className = 'card-sub';
        desc.textContent = description;
        card.appendChild(desc);
      }
      return card;
    }

    function getByPath(target, path) {
      return String(path || '')
        .split('.')
        .filter(Boolean)
        .reduce((value, key) => (value && value[key] !== undefined ? value[key] : undefined), target);
    }

    function resolveInputOptions(input, info) {
      const inline = Array.isArray(input.options) ? input.options : [];
      if (inline.length > 0) return inline.map(String);
      const sourced = input.optionsSource ? getByPath(info, input.optionsSource) : undefined;
      return Array.isArray(sourced) ? sourced.map(String) : [];
    }

    function stableValueKey(value) {
      return JSON.stringify(value);
    }

    function inferValueKind(input, value) {
      const sample = value !== undefined && value !== null ? value : input.default;
      if (typeof sample === 'number') return 'number';
      if (typeof sample === 'boolean') return 'boolean';
      return 'string';
    }

    function applyDirtyState(container, dirty) {
      container.dataset.dirty = dirty ? 'true' : 'false';
      container.classList.toggle('is-dirty', dirty);
      const badge = container.querySelector('.unsaved-badge');
      if (badge) {
        badge.hidden = !dirty;
      }
    }

    function registerDirtyTracker(container, readValue) {
      container.dataset.trackDirty = 'true';
      container.dataset.initialValueKey = stableValueKey(readValue());
      container.__readDirtyValue = readValue;
      applyDirtyState(container, false);
    }

    function refreshDirtyState() {
      const tracked = Array.from(contentViewEl.querySelectorAll('[data-track-dirty="true"]'));
      tracked.forEach(container => {
        if (typeof container.__readDirtyValue !== 'function') return;
        const dirty = stableValueKey(container.__readDirtyValue()) !== container.dataset.initialValueKey;
        applyDirtyState(container, dirty);
      });

      const hasDirty = tracked.some(container => container.dataset.dirty === 'true');
      saveBtn.classList.toggle('is-dirty', hasDirty);
      if (saveBtn.hidden) {
        return;
      }

      const descriptor = currentDescriptor();
      const applyActions = Array.isArray(descriptor && descriptor.settings && descriptor.settings.applyActions)
        ? descriptor.settings.applyActions
        : [];
      const cleanLabel = currentSection === 'core'
        ? 'Save settings'
        : (applyActions.length > 0 ? 'Save and apply' : 'Save settings');
      const dirtyLabel = currentSection === 'core'
        ? 'Save changes'
        : (applyActions.length > 0 ? 'Save and apply changes' : 'Save changes');
      saveBtn.textContent = hasDirty ? dirtyLabel : cleanLabel;
    }

    function coerceControlValue(ctrl, rawValue) {
      const kind = ctrl.dataset.valueKind || 'string';
      if (kind === 'number') {
        return rawValue === '' ? null : Number(rawValue);
      }
      if (kind === 'boolean') {
        return rawValue === 'true';
      }
      return rawValue;
    }

    async function loadJson(url) {
      const res = await copperFetch(url, {
        headers: { 'x-copper-token': model.authToken }
      });
      if (!res.ok) {
        throw new Error((await res.text()) || ('HTTP ' + res.status));
      }
      return await res.json();
    }

    async function refreshDescriptorModel() {
      replaceDescriptorModel(await loadJson('/descriptor'));
    }

    let currentConfig = {};
    let currentInfo = {};
    let currentTabs = [];
    let renderGeneration = 0;


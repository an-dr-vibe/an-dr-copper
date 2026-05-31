pub(super) const CONFIG_UI_SCRIPT_A: &str = r#"
    let descriptors = [];
    let discoverableDescriptors = [];
    let byId = {{}};
    const THEME_OPTIONS = [
      {{ id: 'obsidian-light', label: 'Obsidian Light' }},
      {{ id: 'obsidian-dark', label: 'Obsidian Dark' }},
      {{ id: 'copper', label: 'Copper Dark' }},
      {{ id: 'copper-light', label: 'Copper Light' }},
      {{ id: 'brass', label: 'Brass Dark' }},
      {{ id: 'brass-light', label: 'Brass Light' }},
      {{ id: 'silver', label: 'Silver Dark' }},
      {{ id: 'silver-light', label: 'Silver Light' }},
      {{ id: 'gold', label: 'Gold Dark' }},
      {{ id: 'gold-light', label: 'Gold Light' }},
      {{ id: 'titanium', label: 'Titanium Dark' }},
      {{ id: 'titanium-light', label: 'Titanium Light' }},
    ];
    const THEMES = {{
      'obsidian-light': {{
        bg: '#ffffff', panel: '#f6f6f6', panel2: '#f7f7f7', panel3: '#ffffff',
        line: '#dddddd', text: '#1f1f1f', muted: '#6f6f6f',
        accent: '#705dcf', accentSoft: 'rgba(112,93,207,.14)'
      }},
      'obsidian-dark': {{
        bg: '#1e1e1e', panel: '#262626', panel2: '#242424', panel3: '#1f1f1f',
        line: '#3a3a3a', text: '#dcddde', muted: '#a6a6a6',
        accent: '#8b7cf6', accentSoft: 'rgba(139,124,246,.18)'
      }},
      copper: {{
        bg: '#181210', panel: '#241b18', panel2: '#1d1613', panel3: '#120c0a',
        line: '#533327', text: '#f5ebe4', muted: '#bc9e8f',
        accent: '#d8895a', accentSoft: 'rgba(216,137,90,.18)'
      }},
      'copper-light': {{
        bg: '#fbf3ee', panel: '#fffbf8', panel2: '#f3e1d4', panel3: '#fff6f1',
        line: '#e5c3ad', text: '#43281c', muted: '#916a58',
        accent: '#cb7a4c', accentSoft: 'rgba(203,122,76,.16)'
      }},
      brass: {{
        bg: '#17140f', panel: '#232018', panel2: '#1c1913', panel3: '#100d09',
        line: '#5a4928', text: '#f3ecdd', muted: '#baa97c',
        accent: '#caa24c', accentSoft: 'rgba(202,162,76,.18)'
      }},
      'brass-light': {{
        bg: '#faf5e8', panel: '#fffdf7', panel2: '#f0e4c2', panel3: '#fcf8ef',
        line: '#dfcb90', text: '#403117', muted: '#8a7747',
        accent: '#c89c2f', accentSoft: 'rgba(200,156,47,.15)'
      }},
      silver: {{
        bg: '#13161a', panel: '#1c2026', panel2: '#171b20', panel3: '#0f1216',
        line: '#3d4550', text: '#eff3f7', muted: '#a7b2be',
        accent: '#a6b7ca', accentSoft: 'rgba(166,183,202,.18)'
      }},
      'silver-light': {{
        bg: '#f2f5f8', panel: '#fcfdff', panel2: '#e2e8ee', panel3: '#f6f8fb',
        line: '#cad2db', text: '#27323c', muted: '#697784',
        accent: '#8799ae', accentSoft: 'rgba(135,153,174,.15)'
      }},
      gold: {{
        bg: '#19150e', panel: '#241f15', panel2: '#1d1911', panel3: '#120e08',
        line: '#5a4921', text: '#f7efd8', muted: '#c2ae76',
        accent: '#d8ae3f', accentSoft: 'rgba(216,174,63,.18)'
      }},
      'gold-light': {{
        bg: '#fcf7e7', panel: '#fffdf7', panel2: '#f2e5bb', panel3: '#fdf9ef',
        line: '#e0cd89', text: '#403114', muted: '#8d7941',
        accent: '#cb981c', accentSoft: 'rgba(203,152,28,.15)'
      }},
      titanium: {{
        bg: '#101317', panel: '#191e24', panel2: '#14181d', panel3: '#0c0f13',
        line: '#38424d', text: '#e8eef5', muted: '#98a6b5',
        accent: '#7d92ac', accentSoft: 'rgba(125,146,172,.18)'
      }},
      'titanium-light': {{
        bg: '#eef2f6', panel: '#fbfdff', panel2: '#dce4eb', panel3: '#f4f7fa',
        line: '#c3ced8', text: '#25313c', muted: '#687887',
        accent: '#607b95', accentSoft: 'rgba(96,123,149,.15)'
      }},
    }};

    function normalizeThemeId(themeId) {{
      const normalized = String(themeId || 'obsidian-light').trim().toLowerCase();
      return Object.prototype.hasOwnProperty.call(THEMES, normalized) ? normalized : 'obsidian-light';
    }}

    function resolveTheme(themeId) {{
      const normalized = normalizeThemeId(themeId);
      return THEMES[normalized] || THEMES.copper;
    }}

    function applyTheme(themeId) {{
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
    }}

    function replaceDescriptorModel(nextModel, syncSelection = true) {{
      descriptors = Array.isArray(nextModel.descriptors) ? nextModel.descriptors : [];
      discoverableDescriptors = Array.isArray(nextModel.discoverableDescriptors)
        ? nextModel.discoverableDescriptors
        : descriptors;
      byId = Object.fromEntries(descriptors.map(d => [d.id, d]));
      model.coreExtensionIds = Array.isArray(nextModel.coreExtensionIds)
        ? nextModel.coreExtensionIds
        : (Array.isArray(model.coreExtensionIds) ? model.coreExtensionIds : []);
      model.selectedExtensionId = nextModel.selectedExtensionId || '';
      if (syncSelection && currentSection && currentSection.startsWith('ext:')) {{
        const id = currentSection.slice(4);
        if (!byId[id]) {{
          currentSection = 'core';
          currentTab = '';
          updateUrl();
        }}
      }}
    }}

    replaceDescriptorModel(model, false);
    let currentSection = (() => {{
      const defaultSection = model.selectedExtensionId ? `ext:${{model.selectedExtensionId}}` : 'core';
      const params = new URLSearchParams(window.location.search);
      const requested = params.get('section');
      if (!requested) {{
        return defaultSection;
      }}
      if (requested === 'core') {{
        return 'core';
      }}
      if (requested === 'commands') {{
        return 'commands';
      }}
      if (requested.startsWith('ext:')) {{
        const id = requested.slice(4);
        if (byId[id]) {{
          return requested;
        }}
      }}
      return defaultSection;
    }})();
    let currentTab = (() => {{
      const params = new URLSearchParams(window.location.search);
      return params.get('tab') || '';
    }})();

    const navEl = document.getElementById('nav');
    const contentViewEl = document.getElementById('contentView');
    const statusMsgEl = document.getElementById('statusMsg');
    const closeBtn = document.getElementById('closeBtn');
    const pageEyebrowEl = document.getElementById('pageEyebrow');
    const pageTitleEl = document.getElementById('pageTitle');
    const pageSubEl = document.getElementById('pageSub');
    const tabsEl = document.getElementById('tabs');
    const saveBtn = document.getElementById('saveBtn');

    function currentDescriptor() {{
      return currentSection.startsWith('ext:') ? byId[currentSection.slice(4)] : null;
    }}

    if (!model.allowClose && closeBtn) {{
      closeBtn.style.display = 'none';
    }}

    function setStatus(msg) {{
      statusMsgEl.textContent = msg;
    }}

    function updateUrl() {{
      const params = new URLSearchParams();
      params.set('section', currentSection);
      if (currentTab) {{
        params.set('tab', currentTab);
      }}
      window.history.replaceState(null, '', `${{window.location.pathname}}?${{params.toString()}}`);
    }}

    function humanizeKey(value) {{
      return String(value || '')
        .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
        .replace(/[-_]/g, ' ')
        .replace(/^./, ch => ch.toUpperCase());
    }}

    function formatValue(value, format) {{
      if (value === null || value === undefined || value === '') return 'Not set';
      if (format === 'date-time' && typeof value === 'number') {{
        return new Date(value * 1000).toLocaleString();
      }}
      if (typeof value === 'boolean') return value ? 'Enabled' : 'Disabled';
      if (typeof value === 'object') return JSON.stringify(value);
      return String(value);
    }}

    function createCard(title, description) {{
      const card = document.createElement('section');
      card.className = 'card';
      const heading = document.createElement('h2');
      heading.className = 'card-title';
      heading.textContent = title;
      card.appendChild(heading);
      if (description) {{
        const desc = document.createElement('p');
        desc.className = 'card-sub';
        desc.textContent = description;
        card.appendChild(desc);
      }}
      return card;
    }}

    function getByPath(target, path) {{
      return String(path || '')
        .split('.')
        .filter(Boolean)
        .reduce((value, key) => (value && value[key] !== undefined ? value[key] : undefined), target);
    }}

    function resolveInputOptions(input, info) {{
      const inline = Array.isArray(input.options) ? input.options : [];
      if (inline.length > 0) return inline.map(String);
      const sourced = input.optionsSource ? getByPath(info, input.optionsSource) : undefined;
      return Array.isArray(sourced) ? sourced.map(String) : [];
    }}

    function stableValueKey(value) {{
      return JSON.stringify(value);
    }}

    function inferValueKind(input, value) {{
      const sample = value !== undefined && value !== null ? value : input.default;
      if (typeof sample === 'number') return 'number';
      if (typeof sample === 'boolean') return 'boolean';
      return 'string';
    }}

    function applyDirtyState(container, dirty) {{
      container.dataset.dirty = dirty ? 'true' : 'false';
      container.classList.toggle('is-dirty', dirty);
      const badge = container.querySelector('.unsaved-badge');
      if (badge) {{
        badge.hidden = !dirty;
      }}
    }}

    function registerDirtyTracker(container, readValue) {{
      container.dataset.trackDirty = 'true';
      container.dataset.initialValueKey = stableValueKey(readValue());
      container.__readDirtyValue = readValue;
      applyDirtyState(container, false);
    }}

    function refreshDirtyState() {{
      const tracked = Array.from(contentViewEl.querySelectorAll('[data-track-dirty="true"]'));
      tracked.forEach(container => {{
        if (typeof container.__readDirtyValue !== 'function') return;
        const dirty = stableValueKey(container.__readDirtyValue()) !== container.dataset.initialValueKey;
        applyDirtyState(container, dirty);
      }});

      const hasDirty = tracked.some(container => container.dataset.dirty === 'true');
      saveBtn.classList.toggle('is-dirty', hasDirty);
      if (saveBtn.hidden) {{
        return;
      }}

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
    }}

    function coerceControlValue(ctrl, rawValue) {{
      const kind = ctrl.dataset.valueKind || 'string';
      if (kind === 'number') {{
        return rawValue === '' ? null : Number(rawValue);
      }}
      if (kind === 'boolean') {{
        return rawValue === 'true';
      }}
      return rawValue;
    }}

    function createNavButton(key, label) {{
      const btn = document.createElement('button');
      btn.className = 'nav-btn' + (key === currentSection ? ' active' : '');
      const isCore = key === 'core';
      const isCommands = key === 'commands';
      const navMeta = isCore ? 'Core settings' : isCommands ? 'Run actions' : 'Extension settings';
      btn.innerHTML = `<span class="nav-name">${{label}}</span><span class="nav-meta">${{navMeta}}</span>`;
      btn.addEventListener('click', () => {{
        currentSection = key;
        currentTab = '';
        updateUrl();
        renderNav();
        renderSection().catch(err => setStatus('Load failed: ' + err));
      }});
"#;

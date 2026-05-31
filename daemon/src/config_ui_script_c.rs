pub(super) const CONFIG_UI_SCRIPT_C: &str = r#"
    }}

    function buildStatusRows(statusMeta, status) {{
      const fieldDefs = (statusMeta.fields && statusMeta.fields.length)
        ? statusMeta.fields
        : Object.keys(status).sort().map(key => ({{ key, label: humanizeKey(key) }}));
      return fieldDefs.map(field => ({{
        label: field.label || humanizeKey(field.key),
        value: status[field.key],
        format: field.format
      }}));
    }}

    async function loadJson(url) {{
      const res = await fetch(url, {{
        headers: {{ '{UI_AUTH_HEADER}': model.authToken }}
      }});
      if (!res.ok) {{
        throw new Error((await res.text()) || ('HTTP ' + res.status));
      }}
      return await res.json();
    }}

    async function refreshDescriptorModel() {{
      replaceDescriptorModel(await loadJson('/descriptor'));
    }}

    let currentConfig = {{}};
    let currentInfo = {{}};
    let currentTabs = [];
    let renderGeneration = 0;

    function createCoreExtensionCard(descriptor, enabled) {{
      const card = document.createElement('div');
      card.className = 'extension-card';

      const hidden = document.createElement('input');
      hidden.type = 'hidden';
      hidden.value = enabled ? 'true' : 'false';
      hidden.dataset.inputId = 'extensionEnabled:' + descriptor.id;
      hidden.dataset.inputType = 'extension-toggle';
      card.appendChild(hidden);

      const head = document.createElement('div');
      head.className = 'extension-head';
      const summary = document.createElement('div');
      const titleRow = document.createElement('div');
      titleRow.className = 'extension-title-row';
      const title = document.createElement('div');
      title.className = 'extension-name';
      title.textContent = descriptor.name;
      const dirtyBadge = document.createElement('span');
      dirtyBadge.className = 'unsaved-badge';
      dirtyBadge.textContent = 'Unsaved';
      dirtyBadge.hidden = true;
      const meta = document.createElement('div');
      meta.className = 'extension-id mono';
      meta.textContent = descriptor.id;
      const description = document.createElement('div');
      description.className = 'extension-meta';
      const platforms = Array.isArray(descriptor.platforms) && descriptor.platforms.length > 0
        ? descriptor.platforms.join(', ')
        : 'windows, macos, linux';
      description.textContent = `Platforms: ${{platforms}}`;
      titleRow.appendChild(title);
      titleRow.appendChild(dirtyBadge);
      summary.appendChild(titleRow);
      summary.appendChild(meta);
      summary.appendChild(description);

      const actions = document.createElement('div');
      actions.className = 'extension-actions';
      const state = document.createElement('div');
      state.className = 'toggle-state';
      const enableBtn = document.createElement('button');
      enableBtn.type = 'button';
      enableBtn.className = 'mini-btn';
      enableBtn.textContent = 'Enable';
      const disableBtn = document.createElement('button');
      disableBtn.type = 'button';
      disableBtn.className = 'mini-btn';
      disableBtn.textContent = 'Disable';
      const settingsBtn = document.createElement('button');
      settingsBtn.type = 'button';
      settingsBtn.className = 'mini-btn';
      settingsBtn.textContent = 'Open settings';

      const updateState = () => {{
        const isEnabled = hidden.value === 'true';
        enableBtn.className = 'mini-btn' + (isEnabled ? ' active-enable' : '');
        disableBtn.className = 'mini-btn' + (!isEnabled ? ' active-disable' : '');
        state.textContent = isEnabled ? 'Enabled in runtime' : 'Disabled in runtime';
        settingsBtn.hidden = !isEnabled;
      }};

      enableBtn.addEventListener('click', () => {{
        hidden.value = 'true';
        updateState();
        refreshDirtyState();
      }});
      disableBtn.addEventListener('click', () => {{
        hidden.value = 'false';
        updateState();
        refreshDirtyState();
      }});
      settingsBtn.addEventListener('click', () => {{
        currentSection = `ext:${{descriptor.id}}`;
        currentTab = '';
        updateUrl();
        renderNav();
        renderSection().catch(err => setStatus('Load failed: ' + err));
      }});

      actions.appendChild(enableBtn);
      actions.appendChild(disableBtn);
      actions.appendChild(settingsBtn);
      actions.appendChild(state);
      updateState();

      head.appendChild(summary);
      head.appendChild(actions);
      card.appendChild(head);
      registerDirtyTracker(card, () => hidden.value === 'true');
      return card;
    }}

    async function renderSection() {{
      const generation = ++renderGeneration;
      const sectionAtStart = currentSection;
      contentViewEl.innerHTML = '';
      setStatus('');

      if (sectionAtStart === 'commands') {{
        renderCommandsPage();
        return;
      }}

      function coreSections(config) {{
        const disabledExtensions = new Set(Array.isArray(config.disabledExtensions) ? config.disabledExtensions : []);
        const extensionItems = discoverableDescriptors.map(descriptor => ({{
          descriptor,
          enabled: !disabledExtensions.has(descriptor.id)
        }}));

        return [
          {{
            id: 'general',
            title: 'General',
            description: 'Core Copper configuration.',
            fields: [
              {{
                id: 'userExtensionsDir',
                label: 'User extensions directory',
                description: 'Folder where user-installed extensions are discovered.',
                type: 'text',
                default: '~/.Copper/extensions'
              }},
              {{
                id: 'autoStart',
                label: 'Launch Copper at login',
                description: 'Register or remove Copper autostart for the current user when you save these settings.',
                type: 'boolean',
                default: false
              }},
              {{
                id: 'uiTheme',
                label: 'UI theme',
                description: 'Built-in look and feel for the Copper settings UI.',
                type: 'select',
                options: THEME_OPTIONS.map(theme => theme.id),
                optionLabels: Object.fromEntries(THEME_OPTIONS.map(theme => [theme.id, theme.label])),
                default: 'obsidian-light'
              }}
            ]
          }},
          {{
            id: 'package-install',
            title: 'Package install',
            description: 'Shared extension package installation inputs belong to Copper core settings, not to a torrent workflow extension.',
            fields: [
              {{
                id: 'extensionPackage',
                label: 'Extension package (.zip or .tar.gz)',
                description: 'Package file path used when installing an extension manually.',
                type: 'text',
                default: ''
              }},
              {{
                id: 'extensionsInstallDir',
                label: 'Extensions install directory',
                description: 'Target folder for installed extension packages.',
                type: 'text',
                default: '~/.Copper/extensions'
              }}
            ]
          }},
          {{
            id: 'extensions',
            title: 'Extensions',
            description: 'Extensions can stay discoverable in the UI while being disabled for the active runtime.',
            items: extensionItems
          }}
        ];
      }}

      const configTarget = sectionAtStart === 'core'
        ? '/config/core'
        : '/config/extension/' + encodeURIComponent(sectionAtStart.slice(4));
      const infoTarget = sectionAtStart === 'core'
        ? '/info/core'
        : '/info/extension/' + encodeURIComponent(sectionAtStart.slice(4));
      const [config, info] = await Promise.all([loadJson(configTarget), loadJson(infoTarget)]);
      if (generation !== renderGeneration || sectionAtStart !== currentSection) {{
        return;
      }}
      currentConfig = config || {{}};
      currentInfo = info || {{}};

      if (sectionAtStart === 'core') {{
        pageEyebrowEl.textContent = 'Core';
        pageTitleEl.textContent = 'Copper';
        pageSubEl.textContent = 'Application-wide settings stay separate from extension settings.';
        saveBtn.textContent = 'Save settings';
        applyTheme((config && config.uiTheme) || 'obsidian-light');

        const sections = coreSections(config);
        const coreRows = [
          {{ label: 'Extensions loaded', value: info.extensionsLoaded ?? 0 }},
          {{ label: 'Host platform', value: info.hostPlatform || 'unknown' }},
          {{ label: 'Launch at login', value: config.autoStart ?? false, format: 'boolean' }},
          {{ label: 'User extensions directory', value: info.userExtensionsDir, format: 'path', mono: true }},
          {{ label: 'Core extensions directory', value: info.coreExtensionsDir || 'Not available', format: 'path', mono: true }},
          {{ label: 'Runtime extension roots', value: (info.runtimeExtensionRoots || []).join(', '), format: 'path', mono: true }}
        ];

        currentTabs = [
          {{ id: 'general', title: 'General' }},
          {{ id: 'package-install', title: 'Package Install' }},
          {{ id: 'extensions', title: 'Extensions' }}
        ];
        const generalSection = sections.find(section => section.id === 'general');
        if (generalSection) {{
          const card = createCard(generalSection.title, generalSection.description);
          generalSection.fields.forEach(field => {{
            card.appendChild(createInput(field, config[field.id], info));
          }});
          appendCard(card, 'general', true);
        }}
        const generalStatusHost = document.createElement('div');
        renderKeyValueCard(
          generalStatusHost,
          'Environment',
          'Current Copper environment information.',
          coreRows
        );
        appendCard(generalStatusHost.firstElementChild, 'general', false);

        sections
          .filter(section => section.id !== 'general')
          .forEach(section => {{
            if (section.id === 'extensions') {{
              const card = createCard(section.title, section.description);
              const list = document.createElement('div');
              list.className = 'extension-list';
              (section.items || []).forEach(item => {{
                list.appendChild(createCoreExtensionCard(item.descriptor, item.enabled));
              }});
              card.appendChild(list);
              appendCard(card, section.id, true);
              return;
            }}
            const card = createCard(section.title, section.description);
            section.fields.forEach(field => {{
              card.appendChild(createInput(field, config[field.id], info));
            }});
            appendCard(card, section.id, true);
          }});

        if (!currentTabs.some(tab => tab.id === currentTab)) {{
          currentTab = currentTabs[0].id;
        }}
        renderTabs(currentTabs);
        applyTabVisibility(currentTabs);
        refreshDirtyState();
        return;
      }}

      const extensionId = sectionAtStart.slice(4);
      const descriptor = byId[extensionId];
      if (!descriptor) {{
        throw new Error('Unknown extension section: ' + extensionId);
      }}

      const settingsMeta = descriptor.settings || {{}};
      const applyActions = Array.isArray(settingsMeta.applyActions) ? settingsMeta.applyActions : [];
      pageEyebrowEl.textContent = 'Extension';
      pageTitleEl.textContent = settingsMeta.title || descriptor.name;
      pageSubEl.textContent =
        settingsMeta.description ||
        'Configure this extension in a user-friendly workspace.';
      saveBtn.textContent = applyActions.length > 0 ? 'Save and apply' : 'Save settings';

      const sections = inferSections(descriptor.inputs || [], descriptor);
      const statusMeta = (info && info.statusMeta) || ((settingsMeta || {{}}).status) || {{}};
      const status = (info && info.status) || {{}};
      const statusRows = buildStatusRows(statusMeta, status);
      const declaredTabs = Array.isArray(settingsMeta.tabs)
        ? settingsMeta.tabs.map(tab => normalizeTabSpec(tab, 'Tab')).filter(tab => tab.id)
        : [];
      const sectionToTab = new Map();
      declaredTabs.forEach(tab => {{
        tab.sections.forEach(sectionId => {{
          if (!sectionToTab.has(sectionId)) {{
            sectionToTab.set(sectionId, tab.id);
          }}
        }});
      }});

      currentTabs = declaredTabs.map(tab => ({{
        id: tab.id,
        title: tab.title,
        description: tab.description || ''
      }}));

      if (!declaredTabs.length) {{
        if (sections.length === 0) {{
          const emptyCard = createCard('Settings', 'This extension does not expose editable settings yet.');
          const empty = document.createElement('div');
          empty.className = 'empty';
          empty.textContent = 'No configurable fields were declared in the manifest.';
          emptyCard.appendChild(empty);
          appendCard(emptyCard, '', false);
        }} else {{
          sections.forEach(section => {{
            appendCard(createSettingsCard(section, config, info), '', true);
          }});
        }}
        if (statusRows.length > 0) {{
          const statusHost = document.createElement('div');
          renderKeyValueCard(
            statusHost,
            statusMeta.title || 'Recent status',
            statusMeta.description || 'Latest runtime values reported by the daemon for this extension.',
            statusRows
          );
          appendCard(statusHost.firstElementChild, '', false);
        }}
        currentTab = '';
        renderTabs([]);
        applyTabVisibility([]);
        refreshDirtyState();
        return;
      }}

      const fallbackTabId = declaredTabs[0].id;
      sections.forEach(section => {{
        const tabId = sectionToTab.get(section.id) || fallbackTabId;
        appendCard(createSettingsCard(section, config, info), tabId, true);
      }});

      const statusTab = declaredTabs.find(tab => tab.showStatus);
      if (statusRows.length > 0) {{
        const statusHost = document.createElement('div');
        renderKeyValueCard(
            statusHost,
            statusMeta.title || 'Recent status',
            statusMeta.description || 'Latest runtime values reported by the daemon for this extension.',
            statusRows
        );
        appendCard(statusHost.firstElementChild, statusTab ? statusTab.id : fallbackTabId, false);
      }}

      if (!currentTabs.some(tab => tab.id === currentTab)) {{
        currentTab = currentTabs[0].id;
      }}
      renderTabs(currentTabs);
      applyTabVisibility(currentTabs);
      refreshDirtyState();
    }}
"#;

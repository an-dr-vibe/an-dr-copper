pub(super) const CONFIG_UI_SCRIPT_D: &str = r#"

    function collectCurrentPayload() {{
      if (currentSection === 'commands') return {{}};
      const payload = {{}};
      const remove = [];
      const sameValue = (a, b) => JSON.stringify(a) === JSON.stringify(b);
      const addKey = (id, value, defaultValue) => {{
        const isEmpty = value === '' || value === null || value === undefined;
        if (sameValue(value, defaultValue) || (defaultValue === undefined && isEmpty)) {{
          remove.push(id);
        }} else {{
          payload[id] = value;
        }}
      }};

      const readControlValues = root => {{
        const values = new Map();
        const controls = root.querySelectorAll('[data-input-id]');
        const handled = new Set();
        controls.forEach(ctrl => {{
          const id = ctrl.dataset.inputId;
          if (handled.has(id)) return;
          handled.add(id);
          const type = ctrl.dataset.inputType;
          let value;
          if (type === 'boolean' || type === 'extension-toggle') {{
            value = ctrl.value === 'true';
          }} else if (type === 'multi-select') {{
            value = Array.from(root.querySelectorAll(`[data-input-id="${{id}}"][data-input-type="multi-select"]`))
              .filter(option => option.checked)
              .map(option => option.dataset.optionValue);
          }} else if (type === 'number') {{
            value = ctrl.value === '' ? null : Number(ctrl.value);
          }} else {{
            value = coerceControlValue(ctrl, ctrl.value);
          }}
          values.set(id, value);
        }});
        return values;
      }};

      const visibleValues = readControlValues(contentViewEl);

      if (!currentSection.startsWith('ext:')) {{
        const coreDefaults = {{
          userExtensionsDir: '~/.Copper/extensions',
          autoStart: false,
          uiTheme: 'light',
          disabledExtensions: [],
          extensionPackage: '',
          extensionsInstallDir: '~/.Copper/extensions'
        }};
        Object.keys(coreDefaults).forEach(id => {{
          if (id === 'disabledExtensions') return;
          const value = visibleValues.has(id)
            ? visibleValues.get(id)
            : (currentConfig[id] !== undefined ? currentConfig[id] : coreDefaults[id]);
          addKey(id, value, coreDefaults[id]);
        }});

        const savedDisabled = new Set(
          Array.isArray(currentConfig.disabledExtensions) ? currentConfig.disabledExtensions : []
        );
        const disabledExtensions = discoverableDescriptors
          .filter(descriptor => {{
            const key = 'extensionEnabled:' + descriptor.id;
            const enabled = visibleValues.has(key)
              ? visibleValues.get(key)
              : !savedDisabled.has(descriptor.id);
            return !enabled;
          }})
          .map(descriptor => descriptor.id);
        disabledExtensions.sort();
        addKey('disabledExtensions', disabledExtensions, coreDefaults.disabledExtensions);
        if (remove.length > 0) payload.__remove = remove;
        return payload;
      }}

      const extensionId = currentSection.slice(4);
      const descriptor = byId[extensionId];
      const inputDefaults = {{}};
      (descriptor && descriptor.inputs ? descriptor.inputs : []).forEach(input => {{
        inputDefaults[input.id] = input.default;
      }});
      Object.keys(inputDefaults).forEach(id => {{
        const value = visibleValues.has(id)
          ? visibleValues.get(id)
          : (currentConfig[id] !== undefined ? currentConfig[id] : inputDefaults[id]);
        addKey(id, value, inputDefaults[id]);
      }});
      if (remove.length > 0) payload.__remove = remove;
      return payload;
    }}

    saveBtn.addEventListener('click', async () => {{
      try {{
        const payload = collectCurrentPayload();
        const descriptor = currentDescriptor();
        const applyActions = Array.isArray(descriptor && descriptor.settings && descriptor.settings.applyActions)
          ? descriptor.settings.applyActions
          : [];
        const target = currentSection === 'core'
          ? '/config/core'
          : '/config/extension/' + encodeURIComponent(currentSection.slice(4));
        const res = await fetch(target, {{
          method: 'POST',
          headers: {{
            'content-type': 'application/json',
            '{UI_AUTH_HEADER}': model.authToken
          }},
          body: JSON.stringify(payload)
        }});
        if (!res.ok) {{
          throw new Error(await res.text());
        }}
        if (descriptor && applyActions.length > 0) {{
          const applyRes = await fetch(
            '/apply/extension/' + encodeURIComponent(descriptor.id),
            {{
              method: 'POST',
              headers: {{ '{UI_AUTH_HEADER}': model.authToken }}
            }}
          );
          if (!applyRes.ok) {{
            throw new Error('settings were saved, but apply failed: ' + await applyRes.text());
          }}
          await renderSection();
          setStatus('Settings saved and applied to the current system.');
        }} else {{
          if (currentSection === 'core') {{
            await refreshDescriptorModel();
            renderNav();
          }}
          await renderSection();
          setStatus('Settings saved successfully.');
        }}
      }} catch (err) {{
        setStatus('Save failed: ' + err);
      }}
    }});

    if (closeBtn) {{
      closeBtn.addEventListener('click', async () => {{
        try {{
          await fetch('/close', {{
            method: 'POST',
            headers: {{ '{UI_AUTH_HEADER}': model.authToken }}
          }});
          setStatus('UI server closed. You can close this tab.');
        }} catch (err) {{
          setStatus('Close failed: ' + err);
        }}
      }});
    }}

    applyTheme(model.coreUiTheme || 'light');
    updateUrl();
    renderNav();
"#;

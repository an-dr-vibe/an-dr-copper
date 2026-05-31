pub(super) const CONFIG_UI_SCRIPT_B: &str = r#"
      return btn;
    }}

    function renderNav() {{
      navEl.innerHTML = '';
      navEl.appendChild(createNavButton('core', 'Copper'));
      navEl.appendChild(createNavButton('commands', 'Commands'));
      descriptors.forEach(d => navEl.appendChild(createNavButton(`ext:${{d.id}}`, d.name)));
    }}

    async function runAction(extensionId, actionId) {{
      const res = await fetch(
        '/trigger/extension/' + encodeURIComponent(extensionId) + '/' + encodeURIComponent(actionId),
        {{ method: 'POST', headers: {{ '{UI_AUTH_HEADER}': model.authToken }} }}
      );
      if (!res.ok) throw new Error((await res.text()) || 'HTTP ' + res.status);
      return await res.json();
    }}

    function renderCommandsPage() {{
      pageEyebrowEl.textContent = 'System';
      pageTitleEl.textContent = 'Commands';
      pageSubEl.textContent = 'Run extension actions directly from the UI.';
      saveBtn.hidden = true;
      currentTabs = [];
      renderTabs([]);

      if (!descriptors.length) {{
        const card = createCard('No extensions loaded', '');
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'No extensions are available. Check your extensions directory.';
        card.appendChild(empty);
        contentViewEl.appendChild(card);
        return;
      }}

      descriptors.forEach(descriptor => {{
        const actions = descriptor.actions || [];
        if (!actions.length) return;

        const card = createCard(descriptor.name, descriptor.id);
        const list = document.createElement('div');
        list.className = 'command-run-list';

        actions.forEach(action => {{
          const row = document.createElement('div');
          row.className = 'command-run-row';

          const info = document.createElement('div');
          info.className = 'command-run-info';
          const labelEl = document.createElement('div');
          labelEl.className = 'command-run-label';
          labelEl.textContent = action.label || action.id;
          info.appendChild(labelEl);
          if (action.description) {{
            const descEl = document.createElement('div');
            descEl.className = 'command-run-desc';
            descEl.textContent = action.description;
            info.appendChild(descEl);
          }}

          const runBtn = document.createElement('button');
          runBtn.className = 'run-btn';
          runBtn.textContent = '▶ Run';

          const statusEl = document.createElement('span');
          statusEl.className = 'run-status';

          runBtn.addEventListener('click', async () => {{
            runBtn.disabled = true;
            runBtn.textContent = 'Running…';
            statusEl.textContent = '';
            statusEl.className = 'run-status';
            try {{
              await runAction(descriptor.id, action.id);
              runBtn.textContent = '▶ Run';
              runBtn.className = 'run-btn run-ok';
              statusEl.textContent = '✓ Done';
              statusEl.className = 'run-status run-ok';
              setTimeout(() => {{
                runBtn.className = 'run-btn';
                runBtn.disabled = false;
                statusEl.textContent = '';
                statusEl.className = 'run-status';
              }}, 2000);
            }} catch (err) {{
              runBtn.textContent = '▶ Run';
              runBtn.className = 'run-btn';
              runBtn.disabled = false;
              statusEl.textContent = '✗ ' + (err.message || 'Failed');
              statusEl.className = 'run-status run-error';
            }}
          }});

          row.appendChild(info);
          row.appendChild(runBtn);
          row.appendChild(statusEl);
          list.appendChild(row);
        }});

        card.appendChild(list);
        contentViewEl.appendChild(card);
      }});
    }}

    function createInput(input, value, info) {{
      const wrapper = document.createElement('div');
      wrapper.className = 'input-shell';
      const head = document.createElement('div');
      head.className = 'input-head';
      const label = document.createElement('label');
      label.textContent = input.label;
      const dirtyBadge = document.createElement('span');
      dirtyBadge.className = 'unsaved-badge';
      dirtyBadge.textContent = 'Unsaved';
      dirtyBadge.hidden = true;
      head.appendChild(label);
      head.appendChild(dirtyBadge);
      wrapper.appendChild(head);
      if (input.description) {{
        const help = document.createElement('div');
        help.className = 'field-help';
        help.textContent = input.description;
        wrapper.appendChild(help);
      }}

      let control;
      let readDirtyValue = null;
      if (input.type === 'boolean') {{
        control = document.createElement('select');
        control.innerHTML = '<option value="true">Enabled</option><option value="false">Disabled</option>';
        control.value = String(value ?? input.default ?? false);
        readDirtyValue = () => control.value === 'true';
      }} else if (input.type === 'extension-toggle') {{
        const currentValue = String(value ?? input.default ?? false);
        const hidden = document.createElement('input');
        hidden.type = 'hidden';
        hidden.value = currentValue;
        hidden.dataset.inputId = input.id;
        hidden.dataset.inputType = input.type;
        hidden.dataset.valueKind = 'boolean';

        control = document.createElement('div');
        control.className = 'toggle-control';

        const buttonGroup = document.createElement('div');
        buttonGroup.className = 'toggle-group';
        const enableBtn = document.createElement('button');
        enableBtn.type = 'button';
        enableBtn.className = 'toggle-btn';
        enableBtn.textContent = 'Enable';
        const disableBtn = document.createElement('button');
        disableBtn.type = 'button';
        disableBtn.className = 'toggle-btn';
        disableBtn.textContent = 'Disable';
        const stateText = document.createElement('span');
        stateText.className = 'toggle-state';

        const updateToggleUi = () => {{
          const enabled = hidden.value === 'true';
          enableBtn.className = 'toggle-btn' + (enabled ? ' active-enable' : '');
          disableBtn.className = 'toggle-btn' + (!enabled ? ' active-disable' : '');
          stateText.textContent = enabled ? 'Currently enabled' : 'Currently disabled';
        }};

        enableBtn.addEventListener('click', () => {{
          hidden.value = 'true';
          updateToggleUi();
          refreshDirtyState();
        }});
        disableBtn.addEventListener('click', () => {{
          hidden.value = 'false';
          updateToggleUi();
          refreshDirtyState();
        }});

        buttonGroup.appendChild(enableBtn);
        buttonGroup.appendChild(disableBtn);
        control.appendChild(buttonGroup);
        control.appendChild(stateText);
        control.appendChild(hidden);
        updateToggleUi();
        readDirtyValue = () => hidden.value === 'true';
      }} else if (input.type === 'multi-select') {{
        const options = resolveInputOptions(input, info);
        const selected = Array.isArray(value)
          ? value.map(String)
          : Array.isArray(input.default)
            ? input.default.map(String)
            : [];
        const selectedSet = new Set(selected);
        control = document.createElement('div');
        control.className = 'checkbox-list';
        if (options.length === 0) {{
          const empty = document.createElement('div');
          empty.className = 'empty';
          empty.textContent = 'No options available right now.';
          control.appendChild(empty);
        }} else {{
          options.forEach(opt => {{
            const item = document.createElement('label');
            item.className = 'checkbox-item';
            const checkbox = document.createElement('input');
            checkbox.type = 'checkbox';
            checkbox.checked = selectedSet.has(opt);
            checkbox.dataset.inputId = input.id;
            checkbox.dataset.inputType = input.type;
            checkbox.dataset.optionValue = opt;
            checkbox.addEventListener('change', () => refreshDirtyState());
            const text = document.createElement('span');
            text.textContent = opt;
            item.appendChild(checkbox);
            item.appendChild(text);
            control.appendChild(item);
          }});
        }}
        readDirtyValue = () =>
          Array.from(control.querySelectorAll(`[data-input-id="${{input.id}}"][data-input-type="multi-select"]`))
            .filter(option => option.checked)
            .map(option => option.dataset.optionValue);
      }} else if (input.type === 'list-select') {{
        const options = resolveInputOptions(input, info);
        const selected = String(value ?? input.default ?? options[0] ?? '');
        const hidden = document.createElement('input');
        hidden.type = 'hidden';
        hidden.value = selected;
        hidden.dataset.inputId = input.id;
        hidden.dataset.inputType = input.type;
        hidden.dataset.valueKind = inferValueKind(input, value);

        control = document.createElement('div');
        control.className = 'list-select';
        if (options.length === 0) {{
          const empty = document.createElement('div');
          empty.className = 'empty';
          empty.textContent = 'No options available right now.';
          control.appendChild(empty);
        }} else {{
          const updateListUi = () => {{
            Array.from(control.querySelectorAll('.list-option')).forEach(option => {{
              option.classList.toggle('active', option.dataset.optionValue === hidden.value);
            }});
          }};

          options.forEach(opt => {{
            const option = document.createElement('button');
            option.type = 'button';
            option.className = 'list-option';
            option.dataset.optionValue = opt;
            option.textContent = opt;
            option.addEventListener('click', () => {{
              hidden.value = opt;
              updateListUi();
              refreshDirtyState();
            }});
            control.appendChild(option);
          }});
          updateListUi();
        }}
        control.appendChild(hidden);
        readDirtyValue = () => coerceControlValue(hidden, hidden.value);
      }} else if (input.type === 'select') {{
        control = document.createElement('select');
        resolveInputOptions(input, info).forEach(opt => {{
          const o = document.createElement('option');
          o.value = opt;
          o.textContent = (input.optionLabels && input.optionLabels[opt]) || opt;
          control.appendChild(o);
        }});
        control.dataset.valueKind = inferValueKind(input, value);
        if (input.id === 'uiTheme') {{
          const preferredTheme = value !== undefined && value !== null
            ? value
            : input.default;
          control.value = normalizeThemeId(preferredTheme);
        }} else if (value !== undefined && value !== null) {{
          control.value = String(value);
        }} else if (input.default !== undefined && input.default !== null) {{
          control.value = String(input.default);
        }}
        readDirtyValue = () => coerceControlValue(control, control.value);
        if (input.id === 'uiTheme') {{
          applyTheme(control.value || input.default || 'copper-light');
        }}
      }} else {{
        control = document.createElement('input');
        control.type = (input.type === 'number') ? 'number' : 'text';
        control.value = String(value ?? input.default ?? '');
        readDirtyValue = () =>
          input.type === 'number'
            ? (control.value === '' ? null : Number(control.value))
            : control.value;
      }}

      if (input.type !== 'multi-select' && input.type !== 'extension-toggle' && input.type !== 'list-select') {{
        control.dataset.inputId = input.id;
        control.dataset.inputType = input.type;
        if (input.type === 'select') {{
          control.dataset.valueKind = control.dataset.valueKind || inferValueKind(input, value);
        }}
      }}
      if (input.type === 'number') {{
        control.dataset.valueKind = 'number';
      }}

      if (input.type === 'boolean' || input.type === 'select' || input.type === 'number' || input.type === 'text' || input.type === 'folder-picker' || input.type === 'file-picker') {{
        control.addEventListener('input', () => refreshDirtyState());
        control.addEventListener('change', () => refreshDirtyState());
        if (input.id === 'uiTheme') {{
          control.addEventListener('change', () => applyTheme(normalizeThemeId(control.value)));
        }}
      }}

      wrapper.appendChild(control);
      registerDirtyTracker(wrapper, readDirtyValue || (() => null));
      return wrapper;
    }}

    function inferSections(inputs, descriptor) {{
      const byInputId = Object.fromEntries((inputs || []).map(input => [input.id, input]));
      const declared = (((descriptor || {{}}).settings || {{}}).sections || []);
      const used = new Set();
      const sections = [];

      declared.forEach(section => {{
        const inputDefs = (section.inputs || []).map(id => byInputId[id]).filter(Boolean);
        inputDefs.forEach(input => used.add(input.id));
        if (inputDefs.length > 0) {{
          sections.push({{
            id: section.id,
            title: section.title,
            description: section.description || '',
            inputDefs
          }});
        }}
      }});

      const remaining = (inputs || []).filter(input => !used.has(input.id));
      if (sections.length === 0 && remaining.length > 0) {{
        return [{{ id: 'settings', title: 'Settings', description: '', inputDefs: remaining }}];
      }}
      if (remaining.length > 0) {{
        sections.push({{ id: 'other', title: 'Other settings', description: '', inputDefs: remaining }});
      }}
      return sections;
    }}

    function renderKeyValueCard(target, title, description, rows) {{
      const card = createCard(title, description);
      if (!rows.length) {{
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'Nothing to show yet.';
        card.appendChild(empty);
      }} else {{
        const list = document.createElement('div');
        list.className = 'kv-list';
        rows.forEach(row => {{
          const keyEl = document.createElement('div');
          keyEl.className = 'kv-key';
          keyEl.textContent = row.label;
          const valueEl = document.createElement('div');
          valueEl.className = 'kv-value' + (row.format === 'path' || row.mono ? ' mono' : '');
          valueEl.textContent = formatValue(row.value, row.format);
          list.appendChild(keyEl);
          list.appendChild(valueEl);
        }});
        card.appendChild(list);
      }}
      target.appendChild(card);
      return card;
    }}

    function normalizeTabSpec(tab, fallbackTitle) {{
      return {{
        id: String(tab.id || fallbackTitle || 'tab').trim(),
        title: tab.title || fallbackTitle || 'Tab',
        description: tab.description || '',
        sections: Array.isArray(tab.sections) ? tab.sections.map(String) : [],
        showStatus: Boolean(tab.showStatus)
      }};
    }}

    function renderTabs(tabs) {{
      tabsEl.innerHTML = '';
      tabsEl.hidden = !tabs.length;
      tabs.forEach(tab => {{
        const btn = document.createElement('button');
        btn.className = 'tab-btn' + (currentTab === tab.id ? ' active' : '');
        btn.textContent = tab.title;
        btn.addEventListener('click', () => {{
          currentTab = tab.id;
          updateUrl();
          renderTabs(tabs);
          applyTabVisibility(tabs);
        }});
        tabsEl.appendChild(btn);
      }});
    }}

    function applyTabVisibility(tabs) {{
      const cards = Array.from(contentViewEl.querySelectorAll('[data-tab-id]'));
      if (!tabs.length) {{
        cards.forEach(card => {{
          card.hidden = false;
        }});
        saveBtn.hidden = !contentViewEl.querySelector('[data-editable="true"]');
        updateUrl();
        refreshDirtyState();
        return;
      }}

      const activeTab = tabs.find(tab => tab.id === currentTab) || tabs[0];
      currentTab = activeTab.id;
      cards.forEach(card => {{
        card.hidden = card.dataset.tabId !== currentTab;
      }});
      saveBtn.hidden = !cards.some(card =>
        card.dataset.tabId === currentTab && card.dataset.editable === 'true'
      );
      updateUrl();
      refreshDirtyState();
    }}

    function appendCard(card, tabId, editable = false) {{
      if (tabId) {{
        card.dataset.tabId = tabId;
      }}
      if (editable) {{
        card.dataset.editable = 'true';
      }}
      contentViewEl.appendChild(card);
    }}

    function createSettingsCard(section, config, info) {{
      const card = createCard(section.title, section.description);
      section.inputDefs.forEach(input => {{
        card.appendChild(createInput(input, config[input.id], info));
      }});
      return card;
"#;

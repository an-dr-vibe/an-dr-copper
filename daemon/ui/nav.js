    function createNavButton(key, label) {
      const btn = document.createElement('button');
      btn.className = 'nav-btn' + (key === currentSection ? ' active' : '');
      const isCore = key === 'core';
      const isCommands = key === 'commands';
      const navMeta = isCore ? 'Core settings' : isCommands ? 'Run actions' : 'Extension settings';
      btn.innerHTML = `<span class="nav-name">${label}</span><span class="nav-meta">${navMeta}</span>`;
      btn.addEventListener('click', () => {
        currentSection = key;
        currentTab = '';
        updateUrl();
        renderNav();
        renderSection().catch(err => setStatus('Load failed: ' + err));
      });
      return btn;
    }

    function renderNav() {
      navEl.innerHTML = '';
      const appendGroup = (title, children) => {
        if (!children.length) return;
        const group = document.createElement('section');
        group.className = 'nav-section';
        const heading = document.createElement('div');
        heading.className = 'nav-section-title';
        heading.textContent = title;
        group.appendChild(heading);
        children.forEach(child => group.appendChild(child));
        navEl.appendChild(group);
      };
      const coreIds = new Set(Array.isArray(model.coreExtensionIds) ? model.coreExtensionIds : []);
      const coreExtensionButtons = [];
      const otherExtensionButtons = [];
      descriptors.forEach(d => {
        const button = createNavButton(`ext:${d.id}`, d.name);
        if (coreIds.has(d.id)) {
          coreExtensionButtons.push(button);
        } else {
          otherExtensionButtons.push(button);
        }
      });
      appendGroup('Copper', [
        createNavButton('core', 'Settings'),
        createNavButton('commands', 'Commands')
      ]);
      appendGroup('Core Extensions', coreExtensionButtons);
      appendGroup('Other Extensions', otherExtensionButtons);
    }

    async function runAction(extensionId, actionId) {
      const res = await fetch(
        '/trigger/extension/' + encodeURIComponent(extensionId) + '/' + encodeURIComponent(actionId),
        { method: 'POST', headers: { 'x-copper-token': model.authToken } }
      );
      if (!res.ok) throw new Error((await res.text()) || 'HTTP ' + res.status);
      return await res.json();
    }

    function createCommandRunCard(descriptor, title, description) {
      const actions = descriptor.actions || [];
      const card = createCard(title || descriptor.name, description || descriptor.id);
      const list = document.createElement('div');
      list.className = 'command-run-list';

      actions.forEach(action => {
        const row = document.createElement('div');
        row.className = 'command-run-row';

        const info = document.createElement('div');
        info.className = 'command-run-info';
        const labelEl = document.createElement('div');
        labelEl.className = 'command-run-label';
        labelEl.textContent = action.label || action.id;
        info.appendChild(labelEl);
        if (action.description) {
          const descEl = document.createElement('div');
          descEl.className = 'command-run-desc';
          descEl.textContent = action.description;
          info.appendChild(descEl);
        }

        const runBtn = document.createElement('button');
        runBtn.className = 'run-btn';
        runBtn.textContent = action.id === 'clear' ? 'Clear' : 'Run';

        const statusEl = document.createElement('span');
        statusEl.className = 'run-status';

        runBtn.addEventListener('click', async () => {
          runBtn.disabled = true;
          runBtn.textContent = 'Running...';
          statusEl.textContent = '';
          statusEl.className = 'run-status';
          try {
            await runAction(descriptor.id, action.id);
            runBtn.textContent = action.id === 'clear' ? 'Clear' : 'Run';
            runBtn.className = 'run-btn run-ok';
            statusEl.textContent = 'Done';
            statusEl.className = 'run-status run-ok';
            setTimeout(() => {
              runBtn.className = 'run-btn';
              runBtn.disabled = false;
              statusEl.textContent = '';
              statusEl.className = 'run-status';
            }, 2000);
          } catch (err) {
            runBtn.textContent = action.id === 'clear' ? 'Clear' : 'Run';
            runBtn.className = 'run-btn';
            runBtn.disabled = false;
            statusEl.textContent = err.message || 'Failed';
            statusEl.className = 'run-status run-error';
          }
        });

        row.appendChild(info);
        row.appendChild(runBtn);
        row.appendChild(statusEl);
        list.appendChild(row);
      });

      if (!actions.length) {
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'No commands are available.';
        card.appendChild(empty);
      } else {
        card.appendChild(list);
      }
      return card;
    }

    function renderCommandsPage() {
      pageEyebrowEl.textContent = 'System';
      pageTitleEl.textContent = 'Commands';
      pageSubEl.textContent = 'Run extension actions directly from the UI.';
      saveBtn.hidden = true;
      currentTabs = [];
      renderTabs([]);

      if (!descriptors.length) {
        const card = createCard('No extensions loaded', '');
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'No extensions are available. Check your extensions directory.';
        card.appendChild(empty);
        contentViewEl.appendChild(card);
        return;
      }

      descriptors.forEach(descriptor => {
        const actions = descriptor.actions || [];
        if (!actions.length) return;

        contentViewEl.appendChild(createCommandRunCard(descriptor, descriptor.name, descriptor.id));
      });
    }


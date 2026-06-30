<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy, Key, Link, Money } from '@element-plus/icons-vue'
import PublicHeader from '../../components/common/PublicHeader.vue'
import { useLocale } from '../../composables/useLocale'
import { useSiteBrand } from '../../composables/useSiteBrand'
import { useScrollTo, useCopyText } from '../../composables/usePublicPage'

const { locale, t } = useLocale()
const { siteName } = useSiteBrand()
const scrollToSection = useScrollTo()
const copyDocText = useCopyText()
const apiBaseUrl = computed(() => `${window.location.origin}/v1`)
const anthropicBaseUrl = computed(() => `${window.location.origin}/anthropic`)
const shellInstallScript = computed(() => `curl -fsSL ${window.location.origin}/install | bash`)
const powershellInstallScript = computed(() => `irm ${window.location.origin}/install.ps1 | iex`)
const codexProviderName = computed(() => JSON.stringify(siteName.value))

const claudeInstall = `npm install -g @anthropic-ai/claude-code
claude`

const claudeCodeConfig = computed(
  () => `{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "YOUR_NEOGATE_API_KEY",
    "ANTHROPIC_BASE_URL": "${anthropicBaseUrl.value}",
    "ANTHROPIC_MODEL": "claude-sonnet-4-5",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-sonnet-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-5",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-sonnet-4-5",
    "ANTHROPIC_REASONING_MODEL": "claude-sonnet-4-5",
    "ANTHROPIC_CUSTOM_MODEL_OPTION": "claude-sonnet-4-5"
  },
  "model": "claude-sonnet-4-5"
}`
)

const codexInstall = `npm i -g @openai/codex@latest
codex`

const codexConfig = computed(
  () => `model = "gpt-5.5"
model_provider = "neogate"

[model_providers.neogate]
name = ${codexProviderName.value}
base_url = "${apiBaseUrl.value}"
wire_api = "responses"
requires_openai_auth = false

`
)

const codexAuth = `{
  "OPENAI_API_KEY": "YOUR_NEOGATE_API_KEY",
  "auth_mode": "apikey"
}`

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      title: '帮助文档',
      subtitle: `用 ${siteName.value} 统一管理 API 密钥、余额、用量和上游模型，并通过 OpenAI / Anthropic 兼容接口接入。`,
      menuTitle: '目录',
      menu: [
        ['start', '准备工作', '1. 准备工作'],
        ['auto-config', '自动配置', '2. 自动配置'],
        ['software-config', '手动配置', '3. 手动配置'],
        ['claude-code', '使用 Claude Code', '3.1 使用 Claude Code', 'sub'],
        ['codex', '使用 Codex', '3.2 使用 Codex', 'sub'],
        ['billing', '余额与用量', '4. 余额与用量'],
        ['faq', '常见问题', '5. 常见问题']
      ],
      startTitle: '准备工作',
      startIntro: `第一次使用先取得 ${siteName.value} API Key，并确认要接入的本机工具。准备完成后，可以选择自动配置或手动配置。`,
      startCards: [
        [
          '获取 API 密钥',
          '如果已开放注册，可在首页填写邮箱领取；如果未开放注册，请登录用户后台创建，或联系管理员分配。'
        ],
        [
          '确认客户端',
          '确认要配置 Claude Code 还是 Codex；安装器一次配置一个客户端，并会按客户端显示可用模型。'
        ]
      ],
      installShellTitle: 'Linux / macOS / WSL 自动配置',
      installShellText: `在 bash、zsh 或 WSL 终端中运行下面的命令。脚本会先验证 ${siteName.value} API Key，再选择客户端和模型，并询问是否检查 Node.js、Codex、Claude Code 等依赖。`,
      installWindowsTitle: 'Windows PowerShell 自动配置',
      installWindowsText:
        '在 Windows PowerShell 中运行 install.ps1；如果在 WSL 里使用，请运行上面的 shell 命令。',
      autoConfigIntro: `自动配置会从当前 ${siteName.value} 服务获取安装脚本。脚本验证密钥后会读取可用模型、展示配置摘要，并在你确认后把 Base URL、API Key 和模型名写入本机工具配置。`,
      autoConfigCommandTitle: '2.1 安装命令',
      autoConfigStepsTitle: '2.2 配置步骤',
      autoConfigSteps: [
        [
          '1) 复制并运行安装命令',
          '从首页或本页复制对应系统的安装命令，在终端中执行。',
          '/assets/auto-config-run-command.png',
          `运行 ${siteName.value} 安装命令的终端截图`
        ],
        [
          '2) 按提示确认配置',
          `输入 ${siteName.value} API Key 并通过验证后，选择 Codex CLI 或 Claude Code，再从可用模型列表中选择模型。`,
          '/assets/auto-config-answer-prompts.png',
          '验证 API Key 并选择客户端和模型的终端截图'
        ],
        [
          '3) 写入并测试',
          '确认配置摘要后，按提示检查依赖、写入配置并完成一次网关转发测试；成功后重新运行 codex 或 claude 试用。',
          '/assets/auto-config-complete.png',
          '自动配置完成的终端截图'
        ]
      ],
      switchModelTitle: '2.3 切换模型',
      switchModelIntro: `如果本机已经配置过 ${siteName.value}，再次运行安装命令会尝试读取上次的 API Key、模型和客户端，并提示你切换模型还是重新安装。选择切换模型时，只会重新选择模型、写入配置并做一次转发测试，无需重装依赖。`,
      switchModelStepsTitle: '切换步骤',
      switchModelSteps: [
        '再次运行与首次配置相同的安装命令（Linux/macOS/WSL 用 curl，Windows 用 PowerShell）。',
        '脚本检测到已有配置后会显示菜单：1. 切换模型（默认，回车即可）；2. 重新安装。',
        '回车进入切换流程：确认或自动推断客户端，从可用模型列表中选择新模型（默认高亮上次使用的模型）。',
        '脚本写入新模型配置并执行一次网关转发测试，通过即完成切换；如果要完整重装，选择 2。'
      ],
      manualConfigIntro: `手动配置不依赖安装脚本。先安装目标客户端，再把 ${siteName.value} 的 Base URL、API Key 和模型名写入对应配置文件。`,
      endpointIntro: `下游应用只需要使用 ${siteName.value} 地址和自己的 ${siteName.value} API Key；上游密钥由后台统一管理。`,
      routes: [
        ['OpenAI 模型列表', 'GET', '/v1/models'],
        ['OpenAI Chat Completions', 'POST', '/v1/chat/completions'],
        ['OpenAI Responses', 'POST', '/v1/responses'],
        ['Anthropic 模型列表', 'GET', '/anthropic/v1/messages/models'],
        ['Anthropic Messages', 'POST', '/anthropic/v1/messages']
      ],
      modelListTitle: '测试模型列表',
      requestTitle: '发送一次对话请求',
      pythonTitle: 'Python SDK',
      claudeCodeTitle: 'Claude Code',
      claudeCodeIntro: `如果你准备使用 Claude Code，先安装 Claude Code，再修改用户级 settings.json。这里使用 ${siteName.value} 的 Anthropic 兼容网关和模型名。`,
      installClaudeTitle: '安装 Claude Code',
      openClaudeConfigTitle: '打开或创建配置文件',
      claudeConfigPathText:
        'Claude Code 官方使用 settings.json 管理配置。首次运行 claude 成功后，本地配置目录才会生成；没有 settings.json 时，可以手动创建。',
      claudeConfigPaths: [
        'macOS / Linux / WSL：~/.claude/settings.json',
        'Windows：%USERPROFILE%\\.claude\\settings.json'
      ],
      writeConfigTitle: '写入 settings.json',
      verifyTitle: '验证配置',
      claudeVerifyText:
        '重新运行 claude，进入 Claude Code 后发送一条测试消息；如果能正常回复，说明配置完成。',
      codexTitle: 'Codex',
      codexIntro: `如果你准备使用 Codex，推荐通过 OpenAI 兼容方式接入 ${siteName.value}。`,
      installCodexTitle: '安装 Codex',
      codexConfigFileTitle: '打开配置文件',
      codexConfigFileText:
        'Codex 将配置文件放在用户目录下。按你的系统打开 config.toml；如果文件不存在，请手动创建。模型、提供商和 Base URL 写入 config.toml。',
      codexConfigPaths: [
        'macOS / Linux / WSL：~/.codex/config.toml',
        'Windows：%USERPROFILE%\\.codex\\config.toml'
      ],
      writeCodexConfigTitle: '写入 config.toml',
      codexAuthFileTitle: '打开认证文件',
      codexAuthFileText:
        '按你的系统打开 auth.json；如果文件不存在，请手动创建。API Key 和认证模式写入 auth.json。',
      codexAuthPaths: [
        'macOS / Linux / WSL：~/.codex/auth.json',
        'Windows：%USERPROFILE%\\.codex\\auth.json'
      ],
      writeCodexAuthTitle: '写入 auth.json',
      codexVerifyText: '重新运行 codex。如果能正常进入会话并收到回复，说明配置完成。',
      billingTitle: '余额与用量',
      billingIntro: '登录用户后台后，可以查看账户余额、预留余额、请求记录、Token、费用和延迟。',
      billingItems: [
        ['余额', '可用余额用于真实调用，预留余额用于进行中的请求。'],
        ['用量', '用量页展示模型、Token、费用、首字延迟和总延迟。'],
        ['充值', '余额不足时可以在用户后台选择套餐并查看充值订单。']
      ],
      faqItems: [
        [
          'Token 是什么？怎么计算的？',
          'Token 是 AI 处理文本的基本单位。简单理解：1 个英文单词约等于 1-2 tokens，1 个中文字约等于 1.5-2 tokens。每次对话消耗包括你的输入和 AI 的输出。一次中等复杂度的代码生成对话大约消耗 5,000-10,000 tokens。'
        ],
        [
          '为什么有时消耗特别多？',
          '主要因素包括：对话历史越长消耗越多，因为需要参考上下文；上传大型代码库；生成复杂代码。省 token 技巧：新任务开新对话、精简问题描述、大文件分段处理。'
        ]
      ]
    }
  }

  return {
    title: 'Help Docs',
    subtitle: `Use ${siteName.value} to manage API keys, balance, usage, and upstream models behind OpenAI / Anthropic-compatible APIs.`,
    menuTitle: 'Contents',
    menu: [
      ['start', 'Preparation', '1. Preparation'],
      ['auto-config', 'Automatic Config', '2. Automatic Config'],
      ['software-config', 'Manual Config', '3. Manual Config'],
      ['claude-code', 'Use Claude Code', '3.1 Use Claude Code', 'sub'],
      ['codex', 'Use Codex', '3.2 Use Codex', 'sub'],
      ['billing', 'Billing', '4. Billing'],
      ['faq', 'FAQ', '5. FAQ']
    ],
    startTitle: 'Preparation',
    startIntro: `First get a ${siteName.value} API key and confirm which local tool you want to connect. After preparation, choose automatic or manual configuration.`,
    startCards: [
      [
        'Get an API key',
        'If registration is open, request one from the home page. If registration is closed, create one after signing in or ask an admin to issue it.'
      ],
      [
        'Confirm client',
        'Decide whether to configure Claude Code or Codex. The installer configures one client at a time and lists models for that client.'
      ]
    ],
    installShellTitle: 'Linux / macOS / WSL automatic config',
    installShellText: `Run this command in bash, zsh, or a WSL terminal. The script verifies the ${siteName.value} API key first, then asks for the client and model, and can check dependencies such as Node.js, Codex, and Claude Code.`,
    installWindowsTitle: 'Windows PowerShell automatic config',
    installWindowsText:
      'Run install.ps1 in Windows PowerShell. If you are using WSL, use the shell command above.',
    autoConfigIntro: `Automatic configuration downloads the install script from this ${siteName.value} service. After verifying the key, the script loads available models, shows a config summary, and writes the Base URL, API key, and model name after you confirm.`,
    autoConfigCommandTitle: '2.1 Install Commands',
    autoConfigStepsTitle: '2.2 Steps',
    autoConfigSteps: [
      [
        '1) Copy and run the install command',
        'Copy the command for your operating system from the home page or this page, then run it in a terminal.',
        '/assets/auto-config-run-command.png',
        `Terminal screenshot running the ${siteName.value} install command`
      ],
      [
        '2) Confirm the config',
        `Enter the ${siteName.value} API key, pass verification, select Codex CLI or Claude Code, then choose a model from the available list.`,
        '/assets/auto-config-answer-prompts.png',
        'Terminal screenshot verifying API key and selecting client and model'
      ],
      [
        '3) Write and test',
        'After confirming the summary, follow the prompts to check dependencies, write config, and run one gateway relay test. Then run codex or claude again to try it.',
        '/assets/auto-config-complete.png',
        'Terminal screenshot showing automatic configuration completed'
      ]
    ],
    switchModelTitle: '2.3 Switch Model',
    switchModelIntro: `If your machine is already configured for ${siteName.value}, running the install command again tries to reuse the previous API key, model, and client, then asks whether to switch model or reinstall. Switching only reselects a model, writes it back, and runs a relay test, with no dependency reinstall needed.`,
    switchModelStepsTitle: 'Switch steps',
    switchModelSteps: [
      'Run the same install command you used for the first-time setup (curl on Linux/macOS/WSL, PowerShell on Windows).',
      'When the script detects an existing config it shows a menu: 1. Switch model (default, just press Enter); 2. Reinstall.',
      'Press Enter to enter the switch flow: confirm or infer the client, then pick a new model from the available list (the previously used model is highlighted as the default).',
      'The script writes the new model config and runs a gateway relay test; passing the test completes the switch. Choose 2 for a full reinstall.'
    ],
    manualConfigIntro: `Manual configuration does not use the install script. Install the target client first, then write the ${siteName.value} Base URL, API key, and model name into the matching config file.`,
    endpointIntro: `Apps use ${siteName.value} URLs and a ${siteName.value} API key. Upstream credentials are managed in the admin console.`,
    routes: [
      ['OpenAI models', 'GET', '/v1/models'],
      ['OpenAI Chat Completions', 'POST', '/v1/chat/completions'],
      ['OpenAI Responses', 'POST', '/v1/responses'],
      ['Anthropic models', 'GET', '/anthropic/v1/messages/models'],
      ['Anthropic Messages', 'POST', '/anthropic/v1/messages']
    ],
    modelListTitle: 'List models',
    requestTitle: 'Send one chat request',
    pythonTitle: 'Python SDK',
    claudeCodeTitle: 'Claude Code',
    claudeCodeIntro: `To use Claude Code, install it first, then edit the user-level settings.json file. This uses the ${siteName.value} Anthropic-compatible gateway and model name.`,
    installClaudeTitle: 'Install Claude Code',
    openClaudeConfigTitle: 'Open or create the config file',
    claudeConfigPathText:
      'Claude Code officially uses settings.json for configuration. The local config directory is created after the first successful claude launch. Create settings.json manually if it does not exist.',
    claudeConfigPaths: [
      'macOS / Linux / WSL: ~/.claude/settings.json',
      'Windows: %USERPROFILE%\\.claude\\settings.json'
    ],
    writeConfigTitle: 'Write settings.json',
    verifyTitle: 'Verify setup',
    claudeVerifyText:
      'Run claude again and send one test message. If it replies normally, setup is complete.',
    codexTitle: 'Codex',
    codexIntro: 'To use Codex, connect it through the OpenAI-compatible API.',
    installCodexTitle: 'Install Codex',
    codexConfigFileTitle: 'Open the config file',
    codexConfigFileText:
      'Codex stores its config in your home directory. Open config.toml for your operating system. Create it if it does not exist, then put the model, provider, and Base URL in config.toml.',
    codexConfigPaths: [
      'macOS / Linux / WSL: ~/.codex/config.toml',
      'Windows: %USERPROFILE%\\.codex\\config.toml'
    ],
    writeCodexConfigTitle: 'Write config.toml',
    codexAuthFileTitle: 'Open the auth file',
    codexAuthFileText:
      'Open auth.json for your operating system. Create it if it does not exist, then put the API key and auth mode in auth.json.',
    codexAuthPaths: [
      'macOS / Linux / WSL: ~/.codex/auth.json',
      'Windows: %USERPROFILE%\\.codex\\auth.json'
    ],
    writeCodexAuthTitle: 'Write auth.json',
    codexVerifyText:
      'Run codex again. If it enters a session and receives a reply, setup is complete.',
    billingTitle: 'Billing and usage',
    billingIntro:
      'After signing in, review balance, reserved balance, requests, tokens, cost, and latency.',
    billingItems: [
      [
        'Balance',
        'Available balance is used for calls. Reserved balance covers in-flight requests.'
      ],
      ['Usage', 'Usage records show model, tokens, cost, first-token latency, and total latency.'],
      [
        'Recharge',
        'When balance is low, choose a plan and review recharge orders in the user console.'
      ]
    ],
    faqItems: [
      [
        'What are tokens and how are they calculated?',
        'Tokens are the basic units AI uses to process text. Roughly, one English word is about 1-2 tokens, and one Chinese character is about 1.5-2 tokens. Each conversation consumes both input and output tokens. A medium-complexity code generation conversation may use about 5,000-10,000 tokens.'
      ],
      [
        'Why does usage sometimes become very high?',
        'Common reasons include long conversation history, large codebase uploads, and complex code generation. To save tokens: start a new conversation for new tasks, keep prompts concise, and split large files.'
      ]
    ]
  }
})
</script>

<template>
  <div class="docs-page">
    <PublicHeader header-class="docs-header" />

    <main class="docs-main">
      <section class="docs-hero">
        <h1>{{ content.title }}</h1>
        <p>{{ content.subtitle }}</p>
      </section>

      <div class="docs-layout">
        <aside class="docs-sidebar">
          <h2>{{ content.menuTitle }}</h2>
          <nav>
            <a
              v-for="[id, label, , level] in content.menu"
              :key="id"
              :class="{ 'docs-sidebar-sub-link': level === 'sub' }"
              href=""
              @click.prevent="scrollToSection(id)"
            >
              {{ label }}
            </a>
          </nav>
        </aside>

        <div class="docs-content">
          <section id="start" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[0][2] }}</h2>
              <p>{{ content.startIntro }}</p>
            </div>
            <div class="docs-feature-grid">
              <article
                v-for="[title, text] in content.startCards"
                :key="title"
                class="docs-feature"
              >
                <el-icon><Key v-if="title.includes('API')" /><Link v-else /></el-icon>
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>

          <section id="auto-config" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[1][2] }}</h2>
              <p>{{ content.autoConfigIntro }}</p>
            </div>
            <div class="docs-mini-heading">
              <h3>{{ content.autoConfigCommandTitle }}</h3>
            </div>
            <article class="docs-step-card">
              <h3>{{ content.installShellTitle }}</h3>
              <p>{{ content.installShellText }}</p>
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(shellInstallScript)"
                />
                <pre
                  class="docs-code-sample docs-inner-code"
                ><code>{{ shellInstallScript }}</code></pre>
              </div>
            </article>
            <article class="docs-step-card">
              <h3>{{ content.installWindowsTitle }}</h3>
              <p>{{ content.installWindowsText }}</p>
              <div class="docs-copy-block">
                <el-button
                  :icon="DocumentCopy"
                  text
                  :aria-label="t('copy')"
                  @click="copyDocText(powershellInstallScript)"
                />
                <pre
                  class="docs-code-sample docs-inner-code"
                ><code>{{ powershellInstallScript }}</code></pre>
              </div>
            </article>
            <div class="docs-mini-heading">
              <h3>{{ content.autoConfigStepsTitle }}</h3>
            </div>
            <div class="docs-screenshot-steps">
              <article
                v-for="[title, text, image, alt] in content.autoConfigSteps"
                :key="title"
                class="docs-screenshot-step"
              >
                <div>
                  <h3>{{ title }}</h3>
                  <p>{{ text }}</p>
                </div>
                <img :src="image" :alt="alt" loading="lazy" />
              </article>
            </div>
            <div class="docs-mini-heading">
              <h3>{{ content.switchModelTitle }}</h3>
              <p>{{ content.switchModelIntro }}</p>
            </div>
            <ol class="docs-switch-flow">
              <li v-for="(step, index) in content.switchModelSteps" :key="step">
                <span>{{ index + 1 }}.</span>
                <p>{{ step }}</p>
              </li>
            </ol>
          </section>

          <section id="software-config" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[2][2] }}</h2>
              <p>{{ content.manualConfigIntro }}</p>
            </div>

            <section id="claude-code" class="docs-subsection">
              <div class="docs-section-heading docs-subsection-heading">
                <h2>{{ content.menu[3][2] }}</h2>
                <p>{{ content.claudeCodeIntro }}</p>
              </div>
              <div class="docs-guide-flow">
                <article class="docs-step-card">
                  <h3>{{ content.installClaudeTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(claudeInstall)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ claudeInstall }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.openClaudeConfigTitle }}</h3>
                  <p>{{ content.claudeConfigPathText }}</p>
                  <div
                    class="docs-copy-block"
                    v-for="item in content.claudeConfigPaths"
                    :key="item"
                  >
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(item)"
                    />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ item }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.writeConfigTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(claudeCodeConfig)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ claudeCodeConfig }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.verifyTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText('claude')"
                    />
                    <pre class="docs-code-sample docs-inner-code"><code>claude</code></pre>
                  </div>
                  <p>{{ content.claudeVerifyText }}</p>
                </article>
              </div>
            </section>

            <section id="codex" class="docs-subsection">
              <div class="docs-section-heading docs-subsection-heading">
                <h2>{{ content.menu[4][2] }}</h2>
                <p>{{ content.codexIntro }}</p>
              </div>
              <div class="docs-guide-flow">
                <article class="docs-step-card">
                  <h3>{{ content.installCodexTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(codexInstall)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ codexInstall }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.codexConfigFileTitle }}</h3>
                  <p>{{ content.codexConfigFileText }}</p>
                  <div class="docs-copy-block" v-for="item in content.codexConfigPaths" :key="item">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(item)"
                    />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ item }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.writeCodexConfigTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(codexConfig)"
                    />
                    <pre
                      class="docs-code-sample docs-inner-code"
                    ><code>{{ codexConfig }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.codexAuthFileTitle }}</h3>
                  <p>{{ content.codexAuthFileText }}</p>
                  <div class="docs-copy-block" v-for="item in content.codexAuthPaths" :key="item">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(item)"
                    />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ item }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.writeCodexAuthTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText(codexAuth)"
                    />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ codexAuth }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.verifyTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button
                      :icon="DocumentCopy"
                      text
                      :aria-label="t('copy')"
                      @click="copyDocText('codex')"
                    />
                    <pre class="docs-code-sample docs-inner-code"><code>codex</code></pre>
                  </div>
                  <p>{{ content.codexVerifyText }}</p>
                </article>
              </div>
            </section>
          </section>

          <section id="billing" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[5][2] }}</h2>
              <p>{{ content.billingIntro }}</p>
            </div>
            <div class="docs-check-list">
              <article
                v-for="[title, text] in content.billingItems"
                :key="title"
                class="docs-check-item"
              >
                <el-icon><Money /></el-icon>
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>

          <section id="faq" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[6][2] }}</h2>
            </div>
            <div class="docs-faq-list">
              <article
                v-for="[question, answer] in content.faqItems"
                :key="question"
                class="docs-faq-item"
              >
                <h3>{{ question }}</h3>
                <p>{{ answer }}</p>
              </article>
            </div>
          </section>
        </div>
      </div>
    </main>
  </div>
</template>

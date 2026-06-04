<script setup lang="ts">
import { computed } from 'vue'
import { DocumentCopy, Key, Link, Money, UserFilled } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus/es/components/message/index'
import { useInstallScript } from '../../composables/useInstallScript'
import { useLocale } from '../../composables/useLocale'
import { useAuthStore } from '../../stores/auth'

const githubUrl = 'https://github.com/neogate-io/NeoGate'
const auth = useAuthStore()
const { locale, t, toggleLocale } = useLocale()
const { installScript, copyInstallScript } = useInstallScript(t)
const nextLocaleLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中'))
const dashboardLink = computed(() => (auth.isAdmin ? '/admin' : '/home/overview'))
const apiBaseUrl = computed(() => `${window.location.origin}/v1`)
const anthropicBaseUrl = computed(() => `${window.location.origin}/anthropic`)

const claudeInstall = `npm install -g @anthropic-ai/claude-code
claude`

const claudeCodeConfig = computed(() => `{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "YOUR_NEOGATE_API_KEY",
    "ANTHROPIC_BASE_URL": "${anthropicBaseUrl.value}",
    "ANTHROPIC_MODEL": "gpt-5.5"
  }
}`)

const codexInstall = `npm i -g @openai/codex@latest
codex`

const codexConfig = computed(() => `model = "gpt-5.5"
model_provider = "neogate"

[model_providers.neogate]
name = "NeoGate"
base_url = "${apiBaseUrl.value}"
wire_api = "responses"
requires_openai_auth = false

`)

const codexAuth = `{
  "OPENAI_API_KEY": "YOUR_NEOGATE_API_KEY",
  "auth_mode": "apikey"
}`

function scrollToSection(id: string) {
  document.getElementById(id)?.scrollIntoView({
    behavior: 'smooth',
    block: 'start'
  })
}

async function copyDocText(text: string) {
  await navigator.clipboard.writeText(text)
  ElMessage.success(t('apiKeyCopied'))
}

const content = computed(() => {
  if (locale.value === 'zh-CN') {
    return {
      title: '帮助文档',
      subtitle: '用 NeoGate 统一管理 API 密钥、余额、用量和上游模型，并通过 OpenAI / Anthropic 兼容接口接入。',
      menuTitle: '目录',
      menu: [
        ['start', '开始使用', '1. 开始使用'],
        ['software-config', '软件配置', '2. 软件配置'],
        ['claude-code', '使用 Claude Code', '2.1 使用 Claude Code', 'sub'],
        ['codex', '使用 Codex', '2.2 使用 Codex', 'sub'],
        ['billing', '余额与用量', '3. 余额与用量'],
        ['faq', '常见问题', '4. 常见问题']
      ],
      startTitle: '开始使用',
      startIntro: '第一次使用只需要完成两步：创建 API 密钥，然后执行安装脚本。',
      startCards: [
        ['创建 API 密钥', '在首页填写邮箱创建密钥，完整密钥只会通过邮件发送。'],
        ['执行安装脚本', '复制并运行安装脚本，把 NeoGate 写入本机常用 AI 工具配置。']
      ],
      installTitle: '一键配置本机工具',
      installText: '安装脚本会把当前服务写入常用本地 AI 工具配置。',
      endpointIntro: '下游应用只需要使用 NeoGate 地址和自己的 NeoGate API Key；上游密钥由后台统一管理。',
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
      claudeCodeIntro: '如果你准备使用 Claude Code，先安装 Claude Code，再修改用户级 settings.json。这里使用 NeoGate 的 Anthropic 兼容网关和模型名。',
      installClaudeTitle: '安装 Claude Code',
      openClaudeConfigTitle: '打开或创建配置文件',
      claudeConfigPathText: 'Claude Code 官方使用 settings.json 管理配置。首次运行 claude 成功后，本地配置目录才会生成；没有 settings.json 时，可以手动创建。',
      claudeConfigPaths: ['macOS / Linux / WSL：~/.claude/settings.json', 'Windows：%userprofile%\\.claude\\settings.json'],
      writeConfigTitle: '写入 settings.json',
      verifyTitle: '验证配置',
      claudeVerifyText: '重新运行 claude，进入 Claude Code 后发送一条测试消息；如果能正常回复，说明配置完成。',
      codexTitle: 'Codex',
      codexIntro: '如果你准备使用 Codex，推荐通过 OpenAI 兼容方式接入 NeoGate。',
      installCodexTitle: '安装 Codex',
      codexConfigFileTitle: '打开配置文件',
      codexConfigFileText: '在用户目录下打开 ~/.codex/config.toml；如果文件不存在，请手动创建。模型、提供商和 Base URL 写入 config.toml。',
      writeCodexConfigTitle: '写入 config.toml',
      codexAuthFileTitle: '打开认证文件',
      codexAuthFileText: '在用户目录下打开 ~/.codex/auth.json；如果文件不存在，请手动创建。API Key 和认证模式写入 auth.json。',
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
        ['Token 是什么？怎么计算的？', 'Token 是 AI 处理文本的基本单位。简单理解：1 个英文单词约等于 1-2 tokens，1 个中文字约等于 1.5-2 tokens。每次对话消耗包括你的输入和 AI 的输出。一次中等复杂度的代码生成对话大约消耗 5,000-10,000 tokens。'],
        ['为什么有时消耗特别多？', '主要因素包括：对话历史越长消耗越多，因为需要参考上下文；上传大型代码库；生成复杂代码。省 token 技巧：新任务开新对话、精简问题描述、大文件分段处理。']
      ],
    }
  }

  return {
    title: 'Help Docs',
    subtitle: 'Use NeoGate to manage API keys, balance, usage, and upstream models behind OpenAI / Anthropic-compatible APIs.',
    menuTitle: 'Contents',
    menu: [
      ['start', 'Start', '1. Start'],
      ['software-config', 'Software Config', '2. Software Config'],
      ['claude-code', 'Use Claude Code', '2.1 Use Claude Code', 'sub'],
      ['codex', 'Use Codex', '2.2 Use Codex', 'sub'],
      ['billing', 'Billing', '3. Billing'],
      ['faq', 'FAQ', '4. FAQ']
    ],
    startTitle: 'Start',
    startIntro: 'First-time setup has two steps: create an API key, then run the install script.',
    startCards: [
      ['Create an API key', 'Create a key from the home page. The full key is only sent by email.'],
      ['Run the install script', 'Copy and run the script to write NeoGate into common local AI tool configuration.']
    ],
    installTitle: 'Configure local tools',
    installText: 'The install script writes this service into common local AI tool configuration.',
    endpointIntro: 'Apps use NeoGate URLs and a NeoGate API key. Upstream credentials are managed in the admin console.',
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
      claudeCodeIntro: 'To use Claude Code, install it first, then edit the user-level settings.json file. This uses the NeoGate Anthropic-compatible gateway and model name.',
      installClaudeTitle: 'Install Claude Code',
      openClaudeConfigTitle: 'Open or create the config file',
      claudeConfigPathText: 'Claude Code officially uses settings.json for configuration. The local config directory is created after the first successful claude launch. Create settings.json manually if it does not exist.',
    claudeConfigPaths: ['macOS / Linux / WSL: ~/.claude/settings.json', 'Windows: %userprofile%\\.claude\\settings.json'],
    writeConfigTitle: 'Write settings.json',
    verifyTitle: 'Verify setup',
    claudeVerifyText: 'Run claude again and send one test message. If it replies normally, setup is complete.',
    codexTitle: 'Codex',
    codexIntro: 'To use Codex, connect it through the OpenAI-compatible API.',
    installCodexTitle: 'Install Codex',
    codexConfigFileTitle: 'Open the config file',
    codexConfigFileText: 'Open ~/.codex/config.toml in your home directory. Create it if it does not exist. Put the model, provider, and Base URL in config.toml.',
    writeCodexConfigTitle: 'Write config.toml',
    codexAuthFileTitle: 'Open the auth file',
    codexAuthFileText: 'Open ~/.codex/auth.json in your home directory. Create it if it does not exist. Put the API key and auth mode in auth.json.',
    writeCodexAuthTitle: 'Write auth.json',
    codexVerifyText: 'Run codex again. If it enters a session and receives a reply, setup is complete.',
    billingTitle: 'Billing and usage',
    billingIntro: 'After signing in, review balance, reserved balance, requests, tokens, cost, and latency.',
    billingItems: [
      ['Balance', 'Available balance is used for calls. Reserved balance covers in-flight requests.'],
      ['Usage', 'Usage records show model, tokens, cost, first-token latency, and total latency.'],
      ['Recharge', 'When balance is low, choose a plan and review recharge orders in the user console.']
    ],
    faqItems: [
      ['What are tokens and how are they calculated?', 'Tokens are the basic units AI uses to process text. Roughly, one English word is about 1-2 tokens, and one Chinese character is about 1.5-2 tokens. Each conversation consumes both input and output tokens. A medium-complexity code generation conversation may use about 5,000-10,000 tokens.'],
      ['Why does usage sometimes become very high?', 'Common reasons include long conversation history, large codebase uploads, and complex code generation. To save tokens: start a new conversation for new tasks, keep prompts concise, and split large files.']
    ],
  }
})
</script>

<template>
  <div class="docs-page">
    <header class="home-header docs-header">
      <RouterLink class="home-brand" to="/" :aria-label="t('appName')">
        <img class="home-brand-logo" src="/logo.svg" :alt="t('appName')" />
      </RouterLink>
      <nav class="home-nav" :aria-label="t('appName')">
        <RouterLink to="/">{{ t('home') }}</RouterLink>
        <RouterLink to="/docs">{{ t('docs') }}</RouterLink>
      </nav>
      <div class="home-header-actions">
        <el-button class="home-language-button" :aria-label="t('language')" @click="toggleLocale">
          {{ nextLocaleLabel }}
        </el-button>
        <a class="github-link" :href="githubUrl" target="_blank" rel="noreferrer" :aria-label="t('github')">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="currentColor"
              d="M12 2C6.48 2 2 6.58 2 12.26c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49 0-.24-.01-.88-.01-1.73-2.78.62-3.37-1.38-3.37-1.38-.45-1.19-1.11-1.5-1.11-1.5-.91-.64.07-.63.07-.63 1 .07 1.53 1.06 1.53 1.06.9 1.57 2.35 1.12 2.93.86.09-.67.35-1.12.63-1.38-2.22-.26-4.56-1.14-4.56-5.07 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.33 9.33 0 0 1 12 6.98c.85 0 1.7.12 2.5.34 1.9-1.33 2.74-1.05 2.74-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.94-2.34 4.81-4.57 5.07.36.32.68.95.68 1.92 0 1.38-.01 2.49-.01 2.83 0 .27.18.59.69.49A10.15 10.15 0 0 0 22 12.26C22 6.58 17.52 2 12 2Z"
            />
          </svg>
        </a>
        <RouterLink v-if="auth.isAuthed" class="home-login-link home-account-link" :to="dashboardLink" :aria-label="t('admin')">
          <el-icon><UserFilled /></el-icon>
        </RouterLink>
        <RouterLink v-else class="home-login-link" to="/login">{{ t('signIn') }}</RouterLink>
      </div>
    </header>

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
            <div class="docs-feature-grid docs-start-grid">
              <article v-for="[title, text] in content.startCards" :key="title" class="docs-feature">
                <el-icon><Key v-if="title.includes('API')" /><Link v-else /></el-icon>
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
            <article class="docs-step-card">
              <h3>{{ content.installTitle }}</h3>
              <div class="docs-copy-block">
                <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyInstallScript" />
                <pre class="docs-code-sample docs-inner-code"><code>{{ installScript }}</code></pre>
              </div>
            </article>
          </section>

          <section id="software-config" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[1][2] }}</h2>
            </div>

            <section id="claude-code" class="docs-subsection">
              <div class="docs-section-heading docs-subsection-heading">
                <h2>{{ content.menu[2][2] }}</h2>
                <p>{{ content.claudeCodeIntro }}</p>
              </div>
              <div class="docs-guide-flow">
                <article class="docs-step-card">
                  <h3>{{ content.installClaudeTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(claudeInstall)" />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ claudeInstall }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.openClaudeConfigTitle }}</h3>
                  <p>{{ content.claudeConfigPathText }}</p>
                  <div class="docs-copy-block" v-for="item in content.claudeConfigPaths" :key="item">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(item)" />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ item }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.writeConfigTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(claudeCodeConfig)" />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ claudeCodeConfig }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.verifyTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText('claude')" />
                    <pre class="docs-code-sample docs-inner-code"><code>claude</code></pre>
                  </div>
                  <p>{{ content.claudeVerifyText }}</p>
                </article>
              </div>
            </section>

            <section id="codex" class="docs-subsection">
              <div class="docs-section-heading docs-subsection-heading">
                <h2>{{ content.menu[3][2] }}</h2>
                <p>{{ content.codexIntro }}</p>
              </div>
              <div class="docs-guide-flow">
                <article class="docs-step-card">
                  <h3>{{ content.installCodexTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(codexInstall)" />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ codexInstall }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.codexConfigFileTitle }}</h3>
                  <p>{{ content.codexConfigFileText }}</p>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText('~/.codex/config.toml')" />
                    <pre class="docs-code-sample docs-inner-code"><code>~/.codex/config.toml</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.writeCodexConfigTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(codexConfig)" />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ codexConfig }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.codexAuthFileTitle }}</h3>
                  <p>{{ content.codexAuthFileText }}</p>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText('~/.codex/auth.json')" />
                    <pre class="docs-code-sample docs-inner-code"><code>~/.codex/auth.json</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.writeCodexAuthTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText(codexAuth)" />
                    <pre class="docs-code-sample docs-inner-code"><code>{{ codexAuth }}</code></pre>
                  </div>
                </article>
                <article class="docs-step-card">
                  <h3>{{ content.verifyTitle }}</h3>
                  <div class="docs-copy-block">
                    <el-button :icon="DocumentCopy" text :aria-label="t('copy')" @click="copyDocText('codex')" />
                    <pre class="docs-code-sample docs-inner-code"><code>codex</code></pre>
                  </div>
                  <p>{{ content.codexVerifyText }}</p>
                </article>
              </div>
            </section>
          </section>

          <section id="billing" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[4][2] }}</h2>
              <p>{{ content.billingIntro }}</p>
            </div>
            <div class="docs-check-list">
              <article v-for="[title, text] in content.billingItems" :key="title" class="docs-check-item">
                <el-icon><Money /></el-icon>
                <h3>{{ title }}</h3>
                <p>{{ text }}</p>
              </article>
            </div>
          </section>

          <section id="faq" class="docs-section">
            <div class="docs-section-heading">
              <h2>{{ content.menu[5][2] }}</h2>
            </div>
            <div class="docs-faq-list">
              <article v-for="[question, answer] in content.faqItems" :key="question" class="docs-faq-item">
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

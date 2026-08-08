import { createRouter, createWebHistory } from 'vue-router'
import { getAdminServicePolicy, getSetupStatus, getUserServicePolicy } from '../api/policy'
import type { ServiceMode } from '../api/policy'
import { useAuthStore } from '../stores/auth'
import {
  anthropicSubSections,
  openAiSubSections
} from '../views/public/interfaces/interfacesSections'

declare module 'vue-router' {
  interface RouteMeta {
    allowedServiceModes?: ServiceMode[]
    serviceModeRedirect?: string
  }
}

export const router = createRouter({
  history: createWebHistory(),
  scrollBehavior(to, _from, savedPosition) {
    if (savedPosition) return savedPosition
    if (to.hash) {
      // Section components load asynchronously (defineAsyncComponent), so the
      // hash target may not exist yet right after navigation. Wait briefly for
      // it to mount; fall back to the top of the page if it never appears.
      return new Promise((resolve) => {
        let attempts = 0
        const tryResolve = () => {
          if (document.querySelector(to.hash)) {
            resolve({ el: to.hash, top: 96, behavior: 'smooth' })
            return
          }
          attempts += 1
          if (attempts >= 20) {
            resolve({ top: 0 })
            return
          }
          window.setTimeout(tryResolve, 50)
        }
        tryResolve()
      })
    }
    return { top: 0 }
  },
  routes: [
    {
      path: '/setup',
      name: 'setup',
      component: () => import('../views/setup/SetupView.vue'),
      meta: { messageKey: 'setup' }
    },
    {
      path: '/',
      name: 'home',
      component: () => import('../views/public/HomeView.vue'),
      meta: { messageKey: 'home' }
    },
    {
      path: '/docs',
      name: 'docs',
      component: () => import('../views/public/DocsView.vue'),
      meta: { messageKey: 'docs' }
    },
    {
      path: '/payment/return',
      name: 'paymentReturn',
      component: () => import('../views/public/PaymentReturnView.vue'),
      meta: { messageKey: 'paymentSettings', user: true }
    },
    {
      path: '/interfaces',
      name: 'interfaces',
      redirect: '/interfaces/before-start'
    },
    {
      path: '/interfaces/before-start',
      name: 'interfacesBeforeStart',
      component: () => import('../views/public/InterfacesView.vue'),
      props: { section: 'before-start' },
      meta: { messageKey: 'interfaces' }
    },
    {
      path: `/interfaces/openai/:sub(${openAiSubSections.join('|')})?`,
      name: 'interfacesOpenAi',
      component: () => import('../views/public/InterfacesView.vue'),
      props: (route: { params: { sub?: string } }) => ({
        section: 'openai',
        sub: route.params.sub
      }),
      meta: { messageKey: 'interfaces' }
    },
    {
      path: `/interfaces/anthropic/:sub(${anthropicSubSections.join('|')})?`,
      name: 'interfacesAnthropic',
      component: () => import('../views/public/InterfacesView.vue'),
      props: (route: { params: { sub?: string } }) => ({
        section: 'anthropic',
        sub: route.params.sub
      }),
      meta: { messageKey: 'interfaces' }
    },
    {
      path: '/interfaces/errors',
      name: 'interfacesErrors',
      component: () => import('../views/public/InterfacesView.vue'),
      props: { section: 'errors' },
      meta: { messageKey: 'interfaces' }
    },
    {
      path: '/interfaces/billing',
      name: 'interfacesBilling',
      component: () => import('../views/public/InterfacesView.vue'),
      props: { section: 'billing' },
      meta: { messageKey: 'interfaces' }
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('../views/auth/LoginView.vue'),
      meta: { messageKey: 'admin' }
    },
    {
      path: '/forgot-password',
      name: 'forgotPassword',
      component: () => import('../views/auth/ForgotPasswordView.vue'),
      meta: { messageKey: 'forgotPassword' }
    },
    {
      path: '/reset-password',
      name: 'resetPassword',
      component: () => import('../views/auth/ResetPasswordView.vue'),
      meta: { messageKey: 'resetPassword' }
    },
    {
      path: '/change-password',
      name: 'changePassword',
      component: () => import('../views/auth/ChangePasswordView.vue'),
      meta: { messageKey: 'changePassword', passwordChange: true }
    },
    {
      path: '/home',
      component: () => import('../layouts/UserLayout.vue'),
      meta: { user: true },
      children: [
        { path: '', redirect: '/home/overview' },
        {
          path: 'overview',
          name: 'userOverview',
          component: () => import('../views/user/OverviewView.vue'),
          meta: { messageKey: 'welcomeBack', subtitleKey: 'overviewSubtitle' }
        },
        {
          path: 'apikeys',
          name: 'userApiKeys',
          component: () => import('../views/user/ApiKeysView.vue'),
          meta: { messageKey: 'apiKey', subtitleKey: 'apiKeySubtitle' }
        },
        {
          path: 'recharge',
          name: 'userRecharge',
          component: () => import('../views/user/RechargeView.vue'),
          meta: { messageKey: 'recharge', subtitleKey: 'rechargeSubtitle' }
        },
        {
          path: 'usage',
          name: 'userUsage',
          component: () => import('../views/user/UsageView.vue'),
          meta: { messageKey: 'usage', subtitleKey: 'usageSubtitle' }
        },
        {
          path: 'settings',
          name: 'userSettings',
          component: () => import('../views/user/SettingsView.vue'),
          meta: { messageKey: 'personalSettings', subtitleKey: 'personalSettingsSubtitle' }
        }
      ]
    },
    {
      path: '/admin',
      component: () => import('../layouts/AdminLayout.vue'),
      meta: { admin: true },
      children: [
        { path: '', redirect: '/admin/channels' },
        {
          path: 'apps',
          name: 'apps',
          component: () => import('../views/admin/AppsView.vue'),
          meta: {
            messageKey: 'apps',
            subtitleKey: 'adminAppsSubtitle',
            allowedServiceModes: ['internal'],
            serviceModeRedirect: '/admin/channels'
          }
        },
        {
          path: 'credentials',
          name: 'credentials',
          component: () => import('../views/admin/CredentialsView.vue'),
          meta: {
            messageKey: 'credentialManagement',
            subtitleKey: 'adminCredentialsSubtitle',
            allowedServiceModes: ['paid'],
            serviceModeRedirect: '/admin/channels'
          }
        },
        { path: 'credentials/openai', redirect: '/admin/credentials' },
        {
          path: 'keys',
          name: 'keys',
          component: () => import('../views/admin/UsersView.vue'),
          meta: { messageKey: 'userManagement', subtitleKey: 'adminUsersSubtitle' }
        },
        {
          path: 'projects',
          name: 'projects',
          component: () => import('../views/admin/ProjectsView.vue'),
          meta: {
            messageKey: 'projectManagement',
            subtitleKey: 'adminProjectsSubtitle',
            allowedServiceModes: ['internal'],
            serviceModeRedirect: '/admin/keys'
          }
        },
        {
          path: 'channels',
          name: 'upstreamChannels',
          component: () => import('../views/admin/ChannelsView.vue'),
          meta: { messageKey: 'upstreamChannels', subtitleKey: 'adminChannelsSubtitle' }
        },
        { path: 'prices', redirect: '/admin/channels' },
        {
          path: 'usage',
          name: 'usage',
          component: () => import('../views/admin/UsageView.vue'),
          meta: { messageKey: 'usage', subtitleKey: 'adminUsageSubtitle' }
        },
        {
          path: 'statistics',
          name: 'statistics',
          component: () => import('../views/admin/StatisticsView.vue'),
          meta: {
            messageKey: 'consumptionOverview',
            subtitleKey: 'adminConsumptionOverviewSubtitle'
          }
        },
        {
          path: 'cost-attribution',
          name: 'costAttribution',
          component: () => import('../views/admin/CostAttributionView.vue'),
          meta: {
            messageKey: 'costAttribution',
            subtitleKey: 'adminCostAttributionSubtitle',
            allowedServiceModes: ['internal'],
            serviceModeRedirect: '/admin/statistics'
          }
        },
        { path: 'settings', redirect: '/admin/settings/pricing-policies' },
        {
          path: 'settings/pricing-policies',
          name: 'pricingPolicy',
          component: () => import('../views/admin/PricingSettingsView.vue'),
          meta: { messageKey: 'pricingPolicy', subtitleKey: 'adminPricingSubtitle' }
        },
        {
          path: 'settings/smtp',
          name: 'smtpSettings',
          component: () => import('../views/admin/SmtpSettingsView.vue'),
          meta: { messageKey: 'smtpSettings', subtitleKey: 'adminSmtpSubtitle' }
        },
        {
          path: 'settings/site',
          name: 'siteSettings',
          component: () => import('../views/admin/SiteSettingsView.vue'),
          meta: { messageKey: 'siteSettingsPage', subtitleKey: 'adminSiteSettingsSubtitle' }
        },
        {
          path: 'settings/payment',
          name: 'paymentSettings',
          component: () => import('../views/admin/PaymentSettingsView.vue'),
          meta: {
            messageKey: 'paymentSettings',
            subtitleKey: 'adminPaymentSubtitle',
            allowedServiceModes: ['paid'],
            serviceModeRedirect: '/admin/settings/pricing-policies'
          }
        },
        {
          path: 'settings/other',
          name: 'otherSettings',
          component: () => import('../views/admin/OtherSettingsView.vue'),
          meta: { messageKey: 'otherSettings', subtitleKey: 'adminOtherSettingsSubtitle' }
        },
        {
          path: 'settings/admin-password',
          name: 'adminPasswordSettings',
          component: () => import('../views/admin/PasswordSettingsView.vue'),
          meta: { messageKey: 'adminPasswordSettings', subtitleKey: 'adminPasswordSubtitle' }
        }
      ]
    },
    { path: '/:pathMatch(.*)*', redirect: '/' }
  ]
})

let setupCompleted = false

// Legacy subsection anchors (/interfaces/openai#openai-text) now live on their
// own pages; redirect them before any other navigation guard runs.
router.beforeEach((to) => {
  for (const prefix of ['openai', 'anthropic'] as const) {
    const base = `/interfaces/${prefix}`
    if (to.path === base && to.hash.startsWith(`#${prefix}-`)) {
      return { path: `${base}/${to.hash.slice(prefix.length + 2)}`, replace: true }
    }
  }
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  const setup =
    setupCompleted && to.name !== 'setup' ? null : await getSetupStatus().catch(() => null)
  if (setup?.setup_completed) setupCompleted = true

  if (setup && !setup.setup_completed && to.name !== 'setup') {
    return {
      name: 'setup',
      query: { redirect: to.fullPath }
    }
  }

  if (setup?.setup_completed && to.name === 'setup') {
    return {
      name: 'login'
    }
  }

  const requiresAuth = to.meta.admin === true || to.meta.user === true
  if (requiresAuth && !auth.isAuthed) {
    return {
      name: 'login',
      query: { redirect: to.fullPath }
    }
  }

  if (requiresAuth && !(await auth.verifySession())) {
    return {
      name: 'login',
      query: { redirect: to.fullPath }
    }
  }

  if (
    auth.isUser &&
    auth.requiresPasswordChange &&
    to.name !== 'changePassword' &&
    to.name !== 'login'
  ) {
    return {
      name: 'changePassword',
      query: { redirect: to.fullPath }
    }
  }

  if (
    to.name === 'changePassword' &&
    (!auth.isAuthed || !(await auth.verifySession()) || !auth.isUser)
  ) {
    return {
      name: 'login',
      query: { redirect: to.fullPath }
    }
  }

  if (to.name === 'changePassword' && auth.isUser && !auth.requiresPasswordChange) {
    return '/home/overview'
  }

  if (to.meta.admin === true && !auth.isAdmin) {
    return '/home'
  }

  if (to.meta.user === true && auth.isAdmin) {
    return '/admin'
  }

  if (to.meta.allowedServiceModes?.length && to.meta.serviceModeRedirect) {
    const servicePolicy = await getAdminServicePolicy().catch(() => null)
    if (servicePolicy && !to.meta.allowedServiceModes.includes(servicePolicy.service_mode)) {
      return to.meta.serviceModeRedirect
    }
  }

  if (to.name === 'userRecharge') {
    const servicePolicy = await getUserServicePolicy().catch(() => null)
    if (servicePolicy && !servicePolicy.recharge_enabled) {
      return '/home/overview'
    }
  }

  if (to.name === 'login' && auth.isAuthed && (await auth.verifySession())) {
    if (auth.isUser && auth.requiresPasswordChange) return '/change-password'
    return auth.isAdmin ? '/admin' : '/home/overview'
  }
})

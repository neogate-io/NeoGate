import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '../stores/auth'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'home', component: () => import('../views/public/HomeView.vue'), meta: { messageKey: 'home' } },
    { path: '/docs', name: 'docs', component: () => import('../views/public/DocsView.vue'), meta: { messageKey: 'docs' } },
    { path: '/services', name: 'services', component: () => import('../views/public/ServicesView.vue'), meta: { messageKey: 'services' } },
    { path: '/login', name: 'login', component: () => import('../views/auth/LoginView.vue'), meta: { messageKey: 'admin' } },
    { path: '/forgot-password', name: 'forgotPassword', component: () => import('../views/auth/ForgotPasswordView.vue'), meta: { messageKey: 'forgotPassword' } },
    { path: '/reset-password', name: 'resetPassword', component: () => import('../views/auth/ResetPasswordView.vue'), meta: { messageKey: 'resetPassword' } },
    {
      path: '/home',
      component: () => import('../layouts/UserLayout.vue'),
      meta: { user: true },
      children: [
        { path: '', redirect: '/home/overview' },
        { path: 'overview', name: 'userOverview', component: () => import('../views/user/OverviewView.vue'), meta: { messageKey: 'welcomeBack', subtitleKey: 'overviewSubtitle' } },
        { path: 'apikeys', name: 'userApiKeys', component: () => import('../views/user/ApiKeysView.vue'), meta: { messageKey: 'apiKey', subtitleKey: 'apiKeySubtitle' } },
        { path: 'recharge', name: 'userRecharge', component: () => import('../views/user/RechargeView.vue'), meta: { messageKey: 'recharge', subtitleKey: 'rechargeSubtitle' } },
        { path: 'usage', name: 'userUsage', component: () => import('../views/user/UsageView.vue'), meta: { messageKey: 'usage', subtitleKey: 'usageSubtitle' } }
      ]
    },
    {
      path: '/admin',
      component: () => import('../layouts/AdminLayout.vue'),
      meta: { admin: true },
      children: [
        { path: '', redirect: '/admin/channels' },
        { path: 'credentials', name: 'credentials', component: () => import('../views/admin/CredentialsView.vue'), meta: { messageKey: 'credentialManagement' } },
        { path: 'credentials/openai', redirect: '/admin/credentials' },
        { path: 'keys', name: 'keys', component: () => import('../views/admin/UsersView.vue'), meta: { messageKey: 'userManagement' } },
        { path: 'channels', name: 'upstreamChannels', component: () => import('../views/admin/ChannelsView.vue'), meta: { messageKey: 'upstreamChannels' } },
        { path: 'prices', redirect: '/admin/channels' },
        { path: 'usage', name: 'usage', component: () => import('../views/admin/UsageView.vue'), meta: { messageKey: 'usage' } },
        { path: 'settings', redirect: '/admin/settings/pricing-policies' },
        { path: 'settings/pricing-policies', name: 'pricingPolicy', component: () => import('../views/admin/SettingsView.vue'), meta: { messageKey: 'pricingPolicy' } }
      ]
    },
    { path: '/:pathMatch(.*)*', redirect: '/' }
  ]
})

router.beforeEach((to) => {
  const auth = useAuthStore()

  if ((to.meta.admin === true || to.meta.user === true) && !auth.isAuthed) {
    return {
      name: 'login',
      query: { redirect: to.fullPath }
    }
  }

  if (to.meta.admin === true && !auth.isAdmin) {
    return '/home'
  }

  if (to.meta.user === true && auth.isAdmin) {
    return '/admin'
  }

  if (to.name === 'login' && auth.isAuthed) {
    return auth.isAdmin ? '/admin' : '/home/overview'
  }
})

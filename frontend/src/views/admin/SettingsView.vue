<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { Edit } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getPricingPolicies, upsertPricingPolicy } from '../../api/prices'
import { getAdminServicePolicy, saveAdminServicePolicy } from '../../api/policy'
import { getUserGroups } from '../../api/userKeys'
import { useLocale } from '../../composables/useLocale'
import type { PricingPolicy, UserGroup } from '../../types/admin'
import type { ServicePolicy } from '../../api/policy'
import { readError } from '../../utils/errors'

const { t } = useLocale()

const policies = ref<PricingPolicy[]>([])
const userGroups = ref<UserGroup[]>([])
const loading = ref(false)
const saving = ref(false)
const servicePolicy = ref<ServicePolicy | null>(null)
const servicePolicySaving = ref(false)
const dialogOpen = ref(false)
const editingPolicy = ref<PricingPolicy | null>(null)

const form = reactive({
  userGroup: '',
  multiplierPercent: 100,
  enabled: true
})

const userGroupByCode = computed(() => {
  return new Map(userGroups.value.map((group) => [group.code, group]))
})
const serviceModeLabel = computed(() => {
  if (servicePolicy.value?.service_mode === 'paid') return t('paidServiceMode')
  return t('internalServiceMode')
})
const servicePolicyEditable = computed(() => servicePolicy.value?.service_mode === 'internal')

function rows() {
  return [...policies.value].sort((left, right) => {
    const leftGroup = left.user_group ?? ''
    const rightGroup = right.user_group ?? ''
    const defaultCompare = Number(Boolean(userGroupByCode.value.get(rightGroup)?.is_default))
      - Number(Boolean(userGroupByCode.value.get(leftGroup)?.is_default))
    return defaultCompare || leftGroup.localeCompare(rightGroup)
  })
}

function multiplierPercent(policy: PricingPolicy) {
  return policy.multiplier_micros / 10_000
}

function formatPercent(value: number) {
  return `${value.toLocaleString('en-US', {
    minimumFractionDigits: 0,
    maximumFractionDigits: 2
  })}%`
}

function userGroupName(code?: string | null) {
  if (!code) return '-'
  const group = userGroupByCode.value.get(code)
  return group ? `${group.name} (${group.code})` : code
}

async function load() {
  loading.value = true
  try {
    const [fetchedPolicies, fetchedGroups] = await Promise.all([
      getPricingPolicies(),
      getUserGroups()
    ])
    servicePolicy.value = await getAdminServicePolicy()
    policies.value = fetchedPolicies
    userGroups.value = fetchedGroups
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    loading.value = false
  }
}

async function saveServicePolicy() {
  if (!servicePolicy.value || !servicePolicyEditable.value) return

  servicePolicySaving.value = true
  try {
    servicePolicy.value = await saveAdminServicePolicy({
      credit_required: servicePolicy.value.credit_required
    })
    ElMessage.success(t('servicePolicySaved'))
  } catch (err) {
    ElMessage.error(readError(err))
    servicePolicy.value = await getAdminServicePolicy().catch(() => servicePolicy.value)
  } finally {
    servicePolicySaving.value = false
  }
}

function openEditDialog(policy: PricingPolicy) {
  editingPolicy.value = policy
  Object.assign(form, {
    userGroup: policy.user_group ?? '',
    multiplierPercent: multiplierPercent(policy),
    enabled: policy.enabled
  })
  dialogOpen.value = true
}

async function savePolicy() {
  if (!editingPolicy.value || !form.userGroup) return

  saving.value = true
  try {
    const group = userGroupByCode.value.get(form.userGroup)
    await upsertPricingPolicy({
      id: editingPolicy.value.id,
      name: group?.name ?? editingPolicy.value.name,
      user_group: form.userGroup,
      multiplier_micros: Math.round(form.multiplierPercent * 10_000),
      enabled: form.enabled,
      priority: 0
    })
    ElMessage.success(t('pricingPolicySaved'))
    dialogOpen.value = false
    await load()
  } catch (err) {
    ElMessage.error(readError(err))
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<template>
  <section class="grid">
    <div v-loading="loading" class="admin-settings-panel">
      <div class="settings-panel-header">
        <div>
          <h3>{{ t('servicePolicy') }}</h3>
          <p>{{ t('servicePolicyHint') }}</p>
        </div>
      </div>
      <el-form class="settings-form" label-position="top">
        <el-form-item :label="t('serviceMode')">
          <el-tag type="info" effect="plain">{{ serviceModeLabel }}</el-tag>
        </el-form-item>
        <el-form-item :label="t('creditRequired')">
          <el-switch
            v-if="servicePolicy"
            v-model="servicePolicy.credit_required"
            :disabled="!servicePolicy || !servicePolicyEditable"
          />
          <span class="settings-inline-hint">
            {{
              servicePolicyEditable
                ? t('creditRequiredInternalHint')
                : t('creditRequiredPaidHint')
            }}
          </span>
        </el-form-item>
        <el-button
          v-if="servicePolicyEditable"
          type="primary"
          :loading="servicePolicySaving"
          @click="saveServicePolicy"
        >
          {{ t('save') }}
        </el-button>
      </el-form>
    </div>

    <el-table v-loading="loading" class="admin-table" :data="rows()" stripe>
      <el-table-column :label="t('userGroup')" min-width="220">
        <template #default="{ row }">
          {{ userGroupName(row.user_group) }}
        </template>
      </el-table-column>
      <el-table-column :label="t('pricingPolicyMultiplier')" min-width="160">
        <template #default="{ row }">
          {{ formatPercent(multiplierPercent(row)) }}
        </template>
      </el-table-column>
      <el-table-column :label="t('status')" width="120">
        <template #default="{ row }">
          <el-tag class="static-state-tag" :type="row.enabled ? 'success' : 'info'">
            {{ row.enabled ? t('enabled') : t('disabled') }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column :label="t('actions')" width="120" fixed="right" align="center" header-align="center">
        <template #default="{ row }">
          <el-button class="admin-action-button" :icon="Edit" @click="openEditDialog(row)">
            {{ t('edit') }}
          </el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="dialogOpen" class="settings-dialog" :title="t('editPricingPolicy')" width="460px">
      <el-form class="settings-form" label-position="top">
        <el-form-item :label="t('userGroup')">
          <el-select v-model="form.userGroup" disabled>
            <el-option
              v-for="group in userGroups"
              :key="group.code"
              :label="`${group.name} (${group.code})`"
              :value="group.code"
            />
          </el-select>
        </el-form-item>
        <el-form-item :label="t('pricingPolicyMultiplier')">
          <el-input-number v-model="form.multiplierPercent" :min="0" :precision="2" :step="1" />
        </el-form-item>
        <el-form-item :label="t('enabled')">
          <el-switch v-model="form.enabled" />
        </el-form-item>
      </el-form>

      <template #footer>
        <div class="dialog-footer">
          <el-button @click="dialogOpen = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="saving" @click="savePolicy">{{ t('save') }}</el-button>
        </div>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.dialog-footer {
  display: flex;
  gap: 10px;
  justify-content: flex-end;
}

.admin-settings-panel {
  background: #ffffff;
  border: 1px solid #e5edf5;
  border-radius: 8px;
  display: grid;
  gap: 14px;
  padding: 18px;
}

.settings-panel-header h3 {
  color: #111827;
  font-size: 18px;
  font-weight: 800;
  margin: 0;
}

.settings-panel-header p {
  color: #697586;
  font-size: 13px;
  font-weight: 560;
  line-height: 1.6;
  margin: 6px 0 0;
}

.settings-inline-hint {
  color: #697586;
  font-size: 13px;
  margin-left: 10px;
}

.settings-form :deep(.el-input-number),
.settings-form :deep(.el-select) {
  width: 100%;
}

:global(.settings-dialog) {
  max-width: calc(100vw - 32px);
}
</style>

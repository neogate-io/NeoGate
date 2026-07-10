<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { Edit } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { getPricingPolicies, upsertPricingPolicy } from '../../api/prices'
import { getUserGroups } from '../../api/userKeys'
import { useLocale } from '../../composables/useLocale'
import { withLoading } from '../../composables/useLoadingTask'
import type { PricingPolicy, UserGroup } from '../../types/admin'
import { readError } from '../../utils/errors'

const { locale, t } = useLocale()

const policies = ref<PricingPolicy[]>([])
const userGroups = ref<UserGroup[]>([])
const loading = ref(false)
const saving = ref(false)
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

const pricingRows = computed(() => {
  return [...policies.value].sort((left, right) => {
    const leftGroup = left.user_group ?? ''
    const rightGroup = right.user_group ?? ''
    const defaultCompare =
      Number(Boolean(userGroupByCode.value.get(rightGroup)?.is_default)) -
      Number(Boolean(userGroupByCode.value.get(leftGroup)?.is_default))
    return defaultCompare || leftGroup.localeCompare(rightGroup)
  })
})

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

function userGroupUserCount(code?: string | null) {
  if (!code) return '-'
  return (userGroupByCode.value.get(code)?.user_count ?? 0).toLocaleString(locale.value)
}

async function load() {
  await withLoading(loading, async () => {
    try {
      const [fetchedPolicies, fetchedGroups] = await Promise.all([
        getPricingPolicies(),
        getUserGroups()
      ])
      policies.value = fetchedPolicies
      userGroups.value = fetchedGroups
    } catch (err) {
      ElMessage.error(readError(err))
    }
  })
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
  const policy = editingPolicy.value
  if (!policy || !form.userGroup) return

  await withLoading(saving, async () => {
    try {
      const group = userGroupByCode.value.get(form.userGroup)
      await upsertPricingPolicy({
        id: policy.id,
        name: group?.name ?? policy.name,
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
    }
  })
}

onMounted(load)
</script>

<template>
  <section class="grid admin-page-view">
    <div class="admin-settings-panel">
      <p class="admin-note">{{ t('pricingPolicyHint') }}</p>
    </div>

    <div class="service-table-panel">
      <el-table
        v-loading="loading"
        class="admin-table service-table pricing-policy-table"
        :data="pricingRows"
        stripe
      >
        <el-table-column :label="t('userGroup')" min-width="180">
          <template #default="{ row }">
            {{ userGroupName(row.user_group) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('userCount')" min-width="96">
          <template #default="{ row }">
            {{ userGroupUserCount(row.user_group) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('pricingPolicyMultiplier')" min-width="140">
          <template #default="{ row }">
            {{ formatPercent(multiplierPercent(row)) }}
          </template>
        </el-table-column>
        <el-table-column :label="t('status')" min-width="96">
          <template #default="{ row }">
            <el-tag class="static-state-tag" :type="row.enabled ? 'success' : 'info'">
              {{ row.enabled ? t('enabled') : t('disabled') }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="t('actions')" min-width="64" align="center" header-align="center">
          <template #default="{ row }">
            <div class="table-row-actions">
              <el-button
                class="admin-action-button compact-row-action"
                :aria-label="t('edit')"
                :icon="Edit"
                @click="openEditDialog(row)"
              >
                {{ t('actionEdit') }}
              </el-button>
            </div>
          </template>
        </el-table-column>
        <template #empty>
          <el-empty :description="t('noData')" />
        </template>
      </el-table>
    </div>

    <el-dialog
      v-model="dialogOpen"
      class="settings-dialog"
      :close-on-click-modal="false"
      :title="t('editPricingPolicy')"
      width="400px"
    >
      <el-form class="settings-form" label-position="top">
        <div class="settings-form-row">
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
        </div>
        <el-form-item :label="t('enabled')">
          <el-switch v-model="form.enabled" />
        </el-form-item>
      </el-form>

      <template #footer>
        <div class="admin-dialog-footer">
          <el-button @click="dialogOpen = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="saving" @click="savePolicy">{{
            t('save')
          }}</el-button>
        </div>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.settings-form-row {
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr) 128px;
}

.settings-form :deep(.el-input-number),
.settings-form :deep(.el-select) {
  width: 100%;
}

:global(.settings-dialog) {
  max-width: calc(100vw - 32px);
}

@media (max-width: 560px) {
  .settings-form-row {
    grid-template-columns: 1fr;
  }
}
</style>

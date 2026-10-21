/**
 * 「保存为预设角度」面板：把当前视角存成一条自定义预设。
 *
 * 字段按 issue 的要求给出：名称、投影方式、相机位置、相机角度。
 * 后三项是**只读展示**——这个动作的语义是「把现在看到的画面存下来」，
 * 位置与角度是存进去的内容而不是要填的参数；可编辑的话就成了另一件事
 * （手工构造视角），与「保存为预设」的入口不符。
 */
<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Message } from "@arco-design/web-vue";
import { useAppStore } from "../store";

const { t } = useI18n();

const {
  viewPresetOpen,
  viewer,
  orthoView,
  savedViews,
  saveCurrentView,
} = useAppStore();

/** 输入的名称 */
const name = ref("");

/** 相机位姿：位置与注视目标点，都是三元组 */
type CameraPose = {
  position: [number, number, number];
  target: [number, number, number];
};

/** 当前相机位姿，打开面板时取一次快照。
 *
 * 取快照而不是每次渲染都读：面板打开期间用户若在别处转动了视角，
 * 显示的值应当仍是「点保存那一刻看到的」，否则会存下一个用户没看过的角度。
 */
const pose = ref<CameraPose | null>(null);
/** 存快照时正射与否 */
const wasOrtho = ref(false);

watch(viewPresetOpen, (open) => {
  if (!open) return;
  name.value = "";
  pose.value = (viewer.value?.cameraPose() as CameraPose | undefined) ?? null;
  wasOrtho.value = orthoView.value;
});

/** 由位姿反推的 yaw / pitch。展示用，与写入预设的是同一算法。 */
const angles = computed(() => {
  const p = pose.value;
  if (!p) return null;
  const dx = p.position[0] - p.target[0];
  const dy = p.position[1] - p.target[1];
  const dz = p.position[2] - p.target[2];
  const len = Math.hypot(dx, dy, dz);
  if (len < 1e-6) return null;
  const pitch = (Math.asin(dy / len) * 180) / Math.PI;
  const horiz = Math.hypot(dx, dz);
  let yaw = horiz < 1e-6 ? 0 : (Math.atan2(dx, dz) * 180) / Math.PI + 180;
  yaw = (((yaw + 180) % 360) + 360) % 360 - 180;
  return { yaw, pitch };
});

const canSave = computed(() => name.value.trim().length > 0 && pose.value !== null);

function confirm() {
  const err = saveCurrentView(name.value);
  if (err) {
    Message.warning(err);
    return;
  }
  viewPresetOpen.value = false;
  Message.success(t("view.saved", { name: name.value.trim() }));
}
</script>

<template>
  <a-modal
    v-model:visible="viewPresetOpen"
    :title="t('view.saveTitle')"
    :ok-text="t('view.saveOk')"
    :cancel-text="t('common.cancel')"
    :ok-button-props="{ disabled: !canSave }"
    :mask-closable="false"
    width="440px"
    @ok="confirm"
  >
    <div class="view-preset">
      <a-form :model="{}" layout="vertical" size="small">
        <a-form-item :label="t('view.fieldName')">
          <a-input
            v-model="name"
            :placeholder="t('view.namePlaceholder')"
            :max-length="24"
            allow-clear
            @press-enter="confirm"
          />
        </a-form-item>
        <a-form-item :label="t('view.fieldProjection')">
          <span class="view-preset__value">
            {{ wasOrtho ? t("view.menuOrtho") : t("view.projectionPersp") }}
          </span>
        </a-form-item>
        <a-form-item :label="t('view.fieldPosition')">
          <span class="view-preset__value">
            {{
              pose
                ? pose.position.map((v) => v.toFixed(2)).join(", ")
                : t("view.stateUnavailable")
            }}
          </span>
        </a-form-item>
        <a-form-item :label="t('view.fieldAngles')">
          <span class="view-preset__value">
            {{
              angles
                ? `yaw ${angles.yaw.toFixed(1)}°, pitch ${angles.pitch.toFixed(1)}°`
                : t("view.stateUnavailable")
            }}
          </span>
        </a-form-item>
      </a-form>
      <div v-if="savedViews.length > 0" class="view-preset__hint">
        {{ t("view.existingCount", { count: savedViews.length }) }}
      </div>
    </div>
  </a-modal>
</template>

<style scoped lang="scss">
.view-preset {
  &__value {
    font-family: Consolas, "Courier New", monospace;
    color: var(--color-text-2);
  }

  &__hint {
    color: var(--color-text-3);
    font-size: 12px;
  }
}
</style>

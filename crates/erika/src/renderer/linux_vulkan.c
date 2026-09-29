// FFmpeg owns decoder-specific CUDA/VA-API interop. This bridge never maps
// decoded pixels into host memory. Vulkan copies preserve NV12/P010 planes;
// Erika continues to perform color conversion, tone mapping and composition.
#include <libavutil/hwcontext.h>
#include <libavutil/hwcontext_vulkan.h>
#include <libavutil/error.h>
#include <libavutil/mem.h>
#include <libavutil/avutil.h>
#include <vulkan/vulkan.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

typedef struct ErikaLinuxVk {
    AVBufferRef *device_ref, *frames_ref;
    VkDevice device;
    VkQueue queue;
    uint32_t family;
    VkCommandPool pool;
    VkCommandBuffer command;
    VkFence fence;
    PFN_vkCmdPipelineBarrier2KHR pipeline_barrier;
    VkPhysicalDeviceTimelineSemaphoreFeatures timeline;
    VkPhysicalDeviceSynchronization2Features sync2;
    char **dev_extensions, **inst_extensions;
    int dev_count, inst_count;
    char error[256];
} ErikaLinuxVk;

static int fail(ErikaLinuxVk *s, const char *stage, int error) {
    char text[AV_ERROR_MAX_STRING_SIZE];
    av_strerror(error, text, sizeof(text));
    snprintf(s->error, sizeof(s->error), "%s: %s (%d)", stage, text, error);
    return error;
}

void erika_linux_vk_destroy(ErikaLinuxVk *s) {
    if (!s) return;
    av_buffer_unref(&s->frames_ref);
    av_buffer_unref(&s->device_ref); // user-supplied VkDevice is not destroyed
    if (s->fence) vkDestroyFence(s->device, s->fence, NULL);
    if (s->pool) vkDestroyCommandPool(s->device, s->pool, NULL);
    for (int i = 0; i < s->dev_count; i++) av_free(s->dev_extensions[i]);
    for (int i = 0; i < s->inst_count; i++) av_free(s->inst_extensions[i]);
    av_free(s->dev_extensions); av_free(s->inst_extensions); av_free(s);
}

static char **copy_extensions(const char *const *src, int count) {
    char **dst = av_calloc(count + 1, sizeof(*dst));
    if (!dst) return NULL;
    for (int i = 0; i < count; i++) {
        dst[i] = av_strdup(src[i]);
        if (!dst[i]) {
            while (i > 0) av_free(dst[--i]);
            av_free(dst); return NULL;
        }
    }
    return dst;
}

ErikaLinuxVk *erika_linux_vk_create(uint64_t instance, uint64_t physical,
        uint64_t device, uint32_t family, const VkPhysicalDeviceFeatures *features,
        const char *const *dev_exts, int dev_count,
        const char *const *inst_exts, int inst_count, int *error) {
    ErikaLinuxVk *s = av_mallocz(sizeof(*s));
    if (!s) { *error = AVERROR(ENOMEM); return NULL; }
    s->device = (VkDevice)(uintptr_t)device; s->family = family;
    s->pipeline_barrier = (PFN_vkCmdPipelineBarrier2KHR)vkGetDeviceProcAddr(s->device, "vkCmdPipelineBarrier2KHR");
    if (!s->pipeline_barrier) { *error = AVERROR(ENOSYS); goto failed; }
    s->dev_extensions = copy_extensions(dev_exts, dev_count);
    s->dev_count = s->dev_extensions ? dev_count : 0;
    s->inst_extensions = copy_extensions(inst_exts, inst_count);
    s->inst_count = s->inst_extensions ? inst_count : 0;
    if (!s->dev_extensions || !s->inst_extensions) goto oom;
    s->device_ref = av_hwdevice_ctx_alloc(AV_HWDEVICE_TYPE_VULKAN);
    if (!s->device_ref) goto oom;
    AVHWDeviceContext *ctx = (AVHWDeviceContext*)s->device_ref->data;
    AVVulkanDeviceContext *vk = ctx->hwctx;
    vk->inst = (VkInstance)(uintptr_t)instance;
    vk->phys_dev = (VkPhysicalDevice)(uintptr_t)physical;
    vk->act_dev = s->device;
    vk->get_proc_addr = vkGetInstanceProcAddr;
    s->timeline = (VkPhysicalDeviceTimelineSemaphoreFeatures) {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES,
        .timelineSemaphore = VK_TRUE,
    };
    s->sync2 = (VkPhysicalDeviceSynchronization2Features) {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES,
        .pNext = &s->timeline, .synchronization2 = VK_TRUE,
    };
    vk->device_features = (VkPhysicalDeviceFeatures2) {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
        .pNext = &s->sync2, .features = *features,
    };
    vk->enabled_dev_extensions = (const char *const*)s->dev_extensions;
    vk->nb_enabled_dev_extensions = dev_count;
    vk->enabled_inst_extensions = (const char *const*)s->inst_extensions;
    vk->nb_enabled_inst_extensions = inst_count;
    uint32_t count = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(vk->phys_dev, &count, NULL);
    VkQueueFamilyProperties *props = av_calloc(count, sizeof(*props));
    if (!props) goto oom;
    vkGetPhysicalDeviceQueueFamilyProperties(vk->phys_dev, &count, props);
    if (family >= count || !(props[family].queueFlags & VK_QUEUE_COMPUTE_BIT)) {
        av_free(props); *error = AVERROR(ENOSYS); goto failed;
    }
    vk->qf[0] = (AVVulkanDeviceQueueFamily) {.idx = family, .num = 1, .flags = props[family].queueFlags};
    vk->nb_qf = 1;
    av_free(props);
    *error = av_hwdevice_ctx_init(s->device_ref);
    if (*error < 0) goto failed;
    vkGetDeviceQueue(s->device, family, 0, &s->queue);
    VkCommandPoolCreateInfo pool = {.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT, .queueFamilyIndex = family};
    if (vkCreateCommandPool(s->device, &pool, NULL, &s->pool) != VK_SUCCESS) goto external;
    VkCommandBufferAllocateInfo cmd = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = s->pool, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1};
    if (vkAllocateCommandBuffers(s->device, &cmd, &s->command) != VK_SUCCESS) goto external;
    VkFenceCreateInfo fence = {.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    if (vkCreateFence(s->device, &fence, NULL, &s->fence) != VK_SUCCESS) goto external;
    *error = 0; return s;
oom: *error = AVERROR(ENOMEM); goto failed;
external: *error = AVERROR_EXTERNAL;
failed: erika_linux_vk_destroy(s); return NULL;
}

const char *erika_linux_vk_error(ErikaLinuxVk *s) { return s->error; }

int erika_linux_vk_format(const AVFrame *src) {
    if (!src || !src->hw_frames_ctx) return 0;
    AVHWFramesContext *fc = (AVHWFramesContext*)src->hw_frames_ctx->data;
    return fc->sw_format == AV_PIX_FMT_NV12 ? 8 : fc->sw_format == AV_PIX_FMT_P010 ? 10 : 0;
}

static int frames(ErikaLinuxVk *s, const AVFrame *src) {
    AVHWFramesContext *input = (AVHWFramesContext*)src->hw_frames_ctx->data;
    if (s->frames_ref) {
        AVHWFramesContext *old = (AVHWFramesContext*)s->frames_ref->data;
        if (old->width == src->width && old->height == src->height && old->sw_format == input->sw_format) return 0;
        av_buffer_unref(&s->frames_ref);
    }
    AVBufferRef *ref = av_hwframe_ctx_alloc(s->device_ref);
    if (!ref) return fail(s, "allocate Vulkan frames", AVERROR(ENOMEM));
    AVHWFramesContext *fc = (AVHWFramesContext*)ref->data;
    fc->format = AV_PIX_FMT_VULKAN; fc->sw_format = input->sw_format;
    fc->width = src->width; fc->height = src->height;
    AVVulkanFramesContext *vfc = fc->hwctx;
    vfc->flags = AV_VK_FRAME_FLAG_DISABLE_MULTIPLANE;
    vfc->tiling = VK_IMAGE_TILING_OPTIMAL;
    vfc->usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    int ret = av_hwframe_ctx_init(ref);
    if (ret < 0) { av_buffer_unref(&ref); return fail(s, "initialize Vulkan frames", ret); }
    s->frames_ref = ref; return 0;
}

// Caller has drained wgpu work and established COLOR_ATTACHMENT_OPTIMAL for
// both destination images. Every submitted source semaphore is waited AND
// signalled. Restoration and fence completion precede decoder frame release.
int erika_linux_vk_copy(ErikaLinuxVk *s, const AVFrame *src, uint64_t luma, uint64_t chroma) {
    if (!erika_linux_vk_format(src) || (src->format != AV_PIX_FMT_CUDA && src->format != AV_PIX_FMT_VAAPI))
        return fail(s, "only NV12/P010 CUDA or VA-API frames are accepted", AVERROR(ENOSYS));
    // FFmpeg 8.x frees AVVkFrame.internal on a CUDA import failure and then
    // dereferences it again during pool teardown. Never enter that unsafe API.
    // Upstream c29d710cd5d0 fixes this in FFmpeg 9 (libavutil 61).
    // Hardware decode remains available through the explicit CPU fallback.
    if (src->format == AV_PIX_FMT_CUDA && AV_VERSION_MAJOR(avutil_version()) < 61)
        return fail(s, "CUDA/Vulkan transfer requires FFmpeg 9's failure-cleanup fix; FFmpeg 8 hardware decode can use CPU plane fallback", AVERROR(ENOSYS));
    int ret = frames(s, src);
    if (ret < 0) return ret;
    AVFrame *mapped = av_frame_alloc();
    if (!mapped) return fail(s, "allocate mapped frame", AVERROR(ENOMEM));
    mapped->format = AV_PIX_FMT_VULKAN;
    if (src->format == AV_PIX_FMT_VAAPI) {
        mapped->hw_frames_ctx = av_buffer_ref(s->frames_ref);
        if (!mapped->hw_frames_ctx) { ret = AVERROR(ENOMEM); goto done; }
        ret = av_hwframe_map(mapped, src, AV_HWFRAME_MAP_READ | AV_HWFRAME_MAP_DIRECT);
    } else {
        ret = av_hwframe_get_buffer(s->frames_ref, mapped, 0);
        if (ret >= 0) ret = av_hwframe_transfer_data(mapped, src, 0); // CUDA -> Vulkan, GPU memory only
    }
    if (ret < 0) { fail(s, "GPU frame import (no CPU fallback inside bridge)", ret); goto done; }
    AVVkFrame *frame = (AVVkFrame*)mapped->data[0];
    AVHWFramesContext *fc = (AVHWFramesContext*)mapped->hw_frames_ctx->data;
    AVVulkanFramesContext *vfc = fc->hwctx;
    // Separate R/RG images are required; never interpret a multiplanar image
    // or an unsupported DRM modifier as an ordinary single-plane texture.
    const int p010 = erika_linux_vk_format(src) == 10;
    if (!frame || !frame->img[0] || !frame->img[1] || frame->img[2] || !frame->sem[0] || !frame->sem[1]
        || vfc->format[0] != (p010 ? VK_FORMAT_R16_UNORM : VK_FORMAT_R8_UNORM)
        || vfc->format[1] != (p010 ? VK_FORMAT_R16G16_UNORM : VK_FORMAT_R8G8_UNORM)) {
        ret = fail(s, "unsupported Vulkan plane layout", AVERROR(ENOSYS)); goto done;
    }
    vfc->lock_frame(fc, frame);
    for (int i = 0; i < 2; i++) {
        // FFmpeg must have completed any internal queue-family transfer before
        // lending the image. External/foreign ownership has an explicit import.
        if (frame->queue_family[i] != s->family && frame->queue_family[i] < VK_QUEUE_FAMILY_FOREIGN_EXT) {
            vfc->unlock_frame(fc, frame);
            ret = fail(s, "unsupported source queue family", AVERROR(ENOSYS)); goto done;
        }
    }
    VkImage dst[2] = {(VkImage)luma, (VkImage)chroma};
    VkImageMemoryBarrier2 before[4] = {0}, after[4] = {0};
    uint64_t wait_values[2], signal_values[2];
    VkPipelineStageFlags stages[2] = {VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT};
    for (int i = 0; i < 2; i++) {
        wait_values[i] = frame->sem_value[i]; signal_values[i] = wait_values[i] + 1;
        before[i] = (VkImageMemoryBarrier2) {
            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2,
            .srcStageMask = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT, .dstStageMask = VK_PIPELINE_STAGE_2_TRANSFER_BIT,
            .srcAccessMask = frame->access[i], .dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT,
            .oldLayout = frame->layout[i], .newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            .srcQueueFamilyIndex = frame->queue_family[i],
            .dstQueueFamilyIndex = frame->queue_family[i] == VK_QUEUE_FAMILY_IGNORED ? VK_QUEUE_FAMILY_IGNORED : s->family,
            .image = frame->img[i], .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1},
        };
        before[2+i] = (VkImageMemoryBarrier2) {
            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2,
            .srcStageMask = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT, .dstStageMask = VK_PIPELINE_STAGE_2_TRANSFER_BIT,
            .srcAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT, .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
            .oldLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL, .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
            .image = dst[i], .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1},
        };
        for (int j = i; j <= i+2; j += 2) {
            after[j] = before[j];
            after[j].srcAccessMask = before[j].dstAccessMask;
            after[j].dstAccessMask = before[j].srcAccessMask;
            after[j].srcStageMask = before[j].dstStageMask;
            after[j].dstStageMask = before[j].srcStageMask;
            after[j].oldLayout = before[j].newLayout; after[j].newLayout = before[j].oldLayout;
            after[j].srcQueueFamilyIndex = before[j].dstQueueFamilyIndex;
            after[j].dstQueueFamilyIndex = before[j].srcQueueFamilyIndex;
        }
    }
    VkCommandBufferBeginInfo begin = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT};
    if (vkResetCommandBuffer(s->command, 0) != VK_SUCCESS || vkBeginCommandBuffer(s->command, &begin) != VK_SUCCESS) goto submit_failed;
    VkDependencyInfo dependency = {.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO,
        .imageMemoryBarrierCount = 4, .pImageMemoryBarriers = before};
    s->pipeline_barrier(s->command, &dependency);
    for (int i = 0; i < 2; i++) {
        VkImageCopy copy = {
            .srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1},
            .dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1},
            .extent = {i ? (src->width + 1) / 2 : src->width, i ? (src->height + 1) / 2 : src->height, 1},
        };
        vkCmdCopyImage(s->command, frame->img[i], VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                       dst[i], VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &copy);
    }
    dependency.pImageMemoryBarriers = after;
    s->pipeline_barrier(s->command, &dependency);
    if (vkEndCommandBuffer(s->command) != VK_SUCCESS || vkResetFences(s->device, 1, &s->fence) != VK_SUCCESS) goto submit_failed;
    VkTimelineSemaphoreSubmitInfo timeline = {.sType = VK_STRUCTURE_TYPE_TIMELINE_SEMAPHORE_SUBMIT_INFO,
        .waitSemaphoreValueCount = 2, .pWaitSemaphoreValues = wait_values,
        .signalSemaphoreValueCount = 2, .pSignalSemaphoreValues = signal_values};
    VkSubmitInfo submit = {.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO, .pNext = &timeline,
        .waitSemaphoreCount = 2, .pWaitSemaphores = frame->sem, .pWaitDstStageMask = stages,
        .commandBufferCount = 1, .pCommandBuffers = &s->command,
        .signalSemaphoreCount = 2, .pSignalSemaphores = frame->sem};
    if (vkQueueSubmit(s->queue, 1, &submit, s->fence) != VK_SUCCESS) goto submit_failed;
    for (int i = 0; i < 2; i++) frame->sem_value[i] = signal_values[i];
    if (vkWaitForFences(s->device, 1, &s->fence, VK_TRUE, UINT64_MAX) != VK_SUCCESS) goto submit_failed;
    ret = 0;
    vfc->unlock_frame(fc, frame);
    goto done;
submit_failed:
    vkDeviceWaitIdle(s->device); // do not release frames referenced by GPU work
    vfc->unlock_frame(fc, frame);
    ret = fail(s, "Vulkan GPU plane copy", AVERROR_EXTERNAL);
done:
    av_frame_free(&mapped);
    // Failed imports can invalidate a pooled frame's private interop state.
    // Never hand such a frame to a later playback generation.
    if (ret < 0) av_buffer_unref(&s->frames_ref);
    return ret;
}

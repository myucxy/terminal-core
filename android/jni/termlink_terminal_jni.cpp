#include <jni.h>

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <limits>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <utility>
#include <vector>

#include "termlink_terminal.h"

// All Java_* implementation names are private and reached only through RegisterNatives. Android's
// loader sees JNI_OnLoad as the sole bridge entry point.
#undef JNIEXPORT
#define JNIEXPORT __attribute__((visibility("hidden")))

namespace {

constexpr jsize kAllocationWords = 3;
constexpr jsize kConfigV3Words = 14;
constexpr jsize kConfigV4Words = 18;
constexpr jsize kEventWords = 5;
constexpr jsize kMouseWords = 14;
constexpr jsize kInputV1Words = 9;
constexpr jsize kInputV2Words = 18;
constexpr jsize kInputV3Words = 27;
constexpr jsize kMaximumBatchEvents = 256;
constexpr uint64_t kMaximumSnapshotBytes = UINT64_C(64) * 1024U * 1024U;
constexpr jsize kBrokerIdentityBytes = 96;
constexpr jsize kMaximumBrokerCaptures = 64;
constexpr uint64_t kMaximumBrokerCaptureBytes = UINT64_C(16) * 1024U * 1024U;
constexpr uint64_t kMaximumBrokerOutputBytes = UINT64_C(32) * 1024U * 1024U;

static_assert(sizeof(TlTerminalConfigV1) == 40);
static_assert(sizeof(TlTerminalConfigV3) == 88);
static_assert(sizeof(TlTerminalConfigV4) == 112);
static_assert(sizeof(TlTerminalEvent) == 56);
static_assert(sizeof(TlTerminalBuffer) == 24);
static_assert(sizeof(TlTerminalBrokerRequestV1) == 96);
static_assert(sizeof(TlTerminalCheckpointInfo) == 40);
static_assert(sizeof(TlTerminalPresentationInfoV1) == 48);
static_assert(sizeof(TlTerminalInputV1) == 56);
static_assert(sizeof(TlTerminalInputV2) == 96);
static_assert(sizeof(TlTerminalInputV3) == 160);
static_assert(sizeof(TlTerminalMouseInputV1) == 64);
static_assert(sizeof(TlTerminalHistoryCaptureV1) == 48);
static_assert(sizeof(TlTerminalHistoryRequestV1) == 64);
static_assert(sizeof(TlTerminalDeltaContextV1) == 64);
static_assert(sizeof(TlTerminalDeltaStateInfoV1) == 64);
static_assert(sizeof(TlTerminalDeltaJournalConfigV1) == 80);
static_assert(sizeof(TlTerminalDeltaJournalInfoV1) == 168);
static_assert(sizeof(TlTerminalDeltaApplyInfoV1) == 96);

struct TerminalEntry final {
    explicit TerminalEntry(TlTerminal* value) : terminal(value) {}
    ~TerminalEntry() {
        if (terminal != nullptr) {
            tl_terminal_destroy(terminal);
        }
    }

    std::mutex mutex;
    TlTerminal* terminal;
};

struct TerminalDeleter final {
    void operator()(TlTerminal* terminal) const {
        if (terminal != nullptr) {
            tl_terminal_destroy(terminal);
        }
    }
};

using TerminalOwner = std::unique_ptr<TlTerminal, TerminalDeleter>;

std::mutex g_registry_mutex;
std::unordered_map<uint64_t, std::shared_ptr<TerminalEntry>> g_registry;
std::atomic<uint64_t> g_next_token{1};

class BufferOwner final {
  public:
    BufferOwner() = default;
    ~BufferOwner() {
        if (buffer_.data != nullptr) {
            (void)tl_terminal_buffer_free(&buffer_);
        }
    }
    BufferOwner(const BufferOwner&) = delete;
    BufferOwner& operator=(const BufferOwner&) = delete;
    TlTerminalBuffer* out() { return &buffer_; }
    const TlTerminalBuffer& get() const { return buffer_; }

  private:
    TlTerminalBuffer buffer_{TL_TERMINAL_ABI_VERSION, sizeof(TlTerminalBuffer), nullptr,
#if UINTPTR_MAX == UINT32_MAX
                             0,
#endif
                             0};
};

class BrokerOwner final {
  public:
    explicit BrokerOwner(TlTerminal** terminal) : terminal_(terminal) {}
    ~BrokerOwner() {
        if (transaction_ != nullptr) {
            (void)tl_terminal_broker_cancel_v1(&transaction_, terminal_);
        }
    }
    TlTerminalBrokerOutputV1** out() { return &transaction_; }
    TlTerminalBrokerOutputV1* get() const { return transaction_; }
    uint32_t commit(TlTerminalBuffer* effects) {
        return tl_terminal_broker_commit_v1(&transaction_, terminal_, effects);
    }

  private:
    TlTerminal** terminal_;
    TlTerminalBrokerOutputV1* transaction_ = nullptr;
};

class CheckpointOwner final {
  public:
    ~CheckpointOwner() {
        if (checkpoint_ != nullptr) {
            tl_terminal_checkpoint_destroy(checkpoint_);
        }
    }
    CheckpointOwner(const CheckpointOwner&) = delete;
    CheckpointOwner& operator=(const CheckpointOwner&) = delete;
    CheckpointOwner() = default;
    TlTerminalCheckpoint** out() { return &checkpoint_; }
    TlTerminalCheckpoint* get() const { return checkpoint_; }

  private:
    TlTerminalCheckpoint* checkpoint_ = nullptr;
};

class ByteArrayPin final {
  public:
    ByteArrayPin(JNIEnv* env, jbyteArray array, bool owns_local_ref = false)
        : env_(env), array_(array), owns_local_ref_(owns_local_ref) {
        length_ = array_ == nullptr ? 0 : env_->GetArrayLength(array_);
        if (array_ != nullptr && length_ != 0) {
            bytes_ = env_->GetByteArrayElements(array_, nullptr);
        }
    }
    ~ByteArrayPin() {
        if (bytes_ != nullptr) {
            env_->ReleaseByteArrayElements(array_, bytes_, JNI_ABORT);
        }
        if (owns_local_ref_ && array_ != nullptr) {
            env_->DeleteLocalRef(array_);
        }
    }
    ByteArrayPin(const ByteArrayPin&) = delete;
    ByteArrayPin& operator=(const ByteArrayPin&) = delete;
    bool valid() const { return !env_->ExceptionCheck() && (length_ == 0 || bytes_ != nullptr); }
    const uint8_t* data() const { return reinterpret_cast<const uint8_t*>(bytes_); }
    uint64_t size() const { return static_cast<uint64_t>(length_); }

  private:
    JNIEnv* env_;
    jbyteArray array_;
    bool owns_local_ref_;
    jsize length_ = 0;
    jbyte* bytes_ = nullptr;
};

template <typename Function>
jint safe_status(Function&& function) noexcept {
    try {
        return static_cast<jint>(function());
    } catch (...) {
        return static_cast<jint>(TL_STATUS_PANIC);
    }
}

bool exact_length(JNIEnv* env, jarray array, jsize expected) {
    return array != nullptr && env->GetArrayLength(array) == expected;
}

bool minimum_length(JNIEnv* env, jarray array, jsize minimum) {
    return array != nullptr && env->GetArrayLength(array) >= minimum;
}

bool read_longs(JNIEnv* env, jlongArray array, jsize expected, std::vector<jlong>* output) {
    if (!exact_length(env, array, expected)) {
        return false;
    }
    output->resize(static_cast<size_t>(expected));
    env->GetLongArrayRegion(array, 0, expected, output->data());
    return !env->ExceptionCheck();
}

bool write_longs(JNIEnv* env, jlongArray array, const std::vector<jlong>& values) {
    const jsize count = static_cast<jsize>(values.size());
    if (!minimum_length(env, array, count)) {
        return false;
    }
    env->SetLongArrayRegion(array, 0, count, values.data());
    return !env->ExceptionCheck();
}

std::shared_ptr<TerminalEntry> find_terminal(jlong raw_token) {
    if (raw_token <= 0) {
        return nullptr;
    }
    const auto token = static_cast<uint64_t>(raw_token);
    std::lock_guard lock(g_registry_mutex);
    const auto found = g_registry.find(token);
    return found == g_registry.end() ? nullptr : found->second;
}

uint64_t register_terminal(TerminalOwner terminal) {
    // The unique owner covers allocation failure. Once TerminalEntry exists it becomes the owner,
    // including when unordered_map growth throws before the token can be published.
    auto entry = std::make_shared<TerminalEntry>(terminal.get());
    (void)terminal.release();
    for (;;) {
        const uint64_t token = g_next_token.fetch_add(1, std::memory_order_relaxed);
        if (token == 0 || token > static_cast<uint64_t>(std::numeric_limits<jlong>::max())) {
            continue;
        }
        std::lock_guard lock(g_registry_mutex);
        if (g_registry.emplace(token, entry).second) {
            return token;
        }
    }
}

uint32_t destroy_token(jlong raw_token) {
    if (raw_token <= 0) {
        return TL_STATUS_INVALID_ARGUMENT;
    }
    std::shared_ptr<TerminalEntry> entry;
    {
        std::lock_guard lock(g_registry_mutex);
        const auto found = g_registry.find(static_cast<uint64_t>(raw_token));
        if (found == g_registry.end()) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        entry = found->second;
        g_registry.erase(found);
    }
    std::lock_guard entry_lock(entry->mutex);
    if (entry->terminal == nullptr) {
        return TL_STATUS_INVALID_ARGUMENT;
    }
    tl_terminal_destroy(entry->terminal);
    entry->terminal = nullptr;
    return TL_STATUS_OK;
}

uint32_t publish_allocation(JNIEnv* env, jlongArray output, TlTerminal* terminal,
                            uint64_t next_sequence) {
    const uint64_t token = register_terminal(TerminalOwner(terminal));
    const std::vector<jlong> values{static_cast<jlong>(token), static_cast<jlong>(next_sequence),
                                    static_cast<jlong>(next_sequence)};
    if (!write_longs(env, output, values)) {
        (void)destroy_token(static_cast<jlong>(token));
        return TL_STATUS_PANIC;
    }
    return TL_STATUS_OK;
}

uint32_t publish_buffer(JNIEnv* env, jobjectArray output, BufferOwner* owner, uint64_t maximum) {
    if (!minimum_length(env, output, 1)) {
        return TL_STATUS_INVALID_ARGUMENT;
    }
    const TlTerminalBuffer& buffer = owner->get();
    if (buffer.data == nullptr || buffer.len > maximum ||
        buffer.len > static_cast<uint64_t>(std::numeric_limits<jsize>::max())) {
        return TL_STATUS_PANIC;
    }
    const auto length = static_cast<jsize>(buffer.len);
    jbyteArray bytes = env->NewByteArray(length);
    if (bytes == nullptr || env->ExceptionCheck()) {
        return TL_STATUS_PANIC;
    }
    if (length != 0) {
        env->SetByteArrayRegion(bytes, 0, length, reinterpret_cast<const jbyte*>(buffer.data));
    }
    if (!env->ExceptionCheck()) {
        env->SetObjectArrayElement(output, 0, bytes);
    }
    env->DeleteLocalRef(bytes);
    return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_OK;
}

uint32_t load_u32_le(const uint8_t* bytes) {
    return uint32_t(bytes[0]) | (uint32_t(bytes[1]) << 8U) |
           (uint32_t(bytes[2]) << 16U) | (uint32_t(bytes[3]) << 24U);
}

uint64_t load_u64_le(const uint8_t* bytes) {
    return uint64_t(load_u32_le(bytes)) | (uint64_t(load_u32_le(bytes + 4)) << 32U);
}

bool decode_broker_identity(const uint8_t* bytes, TlTerminalBrokerRequestV1* identity) {
    identity->abi_version = load_u32_le(bytes);
    identity->struct_size = load_u32_le(bytes + 4);
    identity->has_request = load_u32_le(bytes + 8);
    identity->medium = load_u32_le(bytes + 12);
    identity->source_sequence = load_u64_le(bytes + 16);
    identity->source_offset_end = load_u32_le(bytes + 24);
    identity->ordinal = load_u32_le(bytes + 28);
    std::copy_n(bytes + 32, 32, identity->control_digest);
    identity->read_offset = load_u64_le(bytes + 64);
    identity->read_length = load_u32_le(bytes + 72);
    identity->has_read_length = load_u32_le(bytes + 76);
    identity->reserved[0] = load_u64_le(bytes + 80);
    identity->reserved[1] = load_u64_le(bytes + 88);
    return identity->abi_version == TL_TERMINAL_ABI_VERSION &&
           identity->struct_size == sizeof(TlTerminalBrokerRequestV1) &&
           identity->has_request == 1 && identity->reserved[0] == 0 &&
           identity->reserved[1] == 0 && identity->has_read_length <= 1;
}

bool same_broker_identity(const TlTerminalBrokerRequestV1& left,
                          const TlTerminalBrokerRequestV1& right) {
    return left.abi_version == right.abi_version && left.struct_size == right.struct_size &&
           left.has_request == right.has_request && left.medium == right.medium &&
           left.source_sequence == right.source_sequence &&
           left.source_offset_end == right.source_offset_end && left.ordinal == right.ordinal &&
           std::equal(left.control_digest, left.control_digest + 32, right.control_digest) &&
           left.read_offset == right.read_offset && left.read_length == right.read_length &&
           left.has_read_length == right.has_read_length && left.reserved[0] == right.reserved[0] &&
           left.reserved[1] == right.reserved[1];
}

template <typename Function>
uint32_t with_terminal(jlong token, Function&& function) {
    auto entry = find_terminal(token);
    if (entry == nullptr) {
        return TL_STATUS_INVALID_ARGUMENT;
    }
    std::lock_guard lock(entry->mutex);
    if (entry->terminal == nullptr) {
        return TL_STATUS_INVALID_ARGUMENT;
    }
    return function(entry->terminal);
}

uint32_t marshal_events(JNIEnv* env, jlongArray raw_events, jobjectArray raw_payloads,
                        std::vector<TlTerminalEvent>* events,
                        std::vector<std::unique_ptr<ByteArrayPin>>* pins) {
    if (raw_events == nullptr || raw_payloads == nullptr) {
        return TL_STATUS_NULL_POINTER;
    }
    const jsize word_count = env->GetArrayLength(raw_events);
    if (word_count <= 0 || word_count % kEventWords != 0) {
        return TL_STATUS_INVALID_ARGUMENT;
    }
    const jsize event_count = word_count / kEventWords;
    if (event_count > kMaximumBatchEvents || env->GetArrayLength(raw_payloads) != event_count) {
        return TL_STATUS_LIMIT_EXCEEDED;
    }
    std::vector<jlong> words(static_cast<size_t>(word_count));
    env->GetLongArrayRegion(raw_events, 0, word_count, words.data());
    if (env->ExceptionCheck()) {
        return TL_STATUS_PANIC;
    }
    events->reserve(static_cast<size_t>(event_count));
    pins->reserve(static_cast<size_t>(event_count));
    for (jsize index = 0; index < event_count; ++index) {
        const size_t offset = static_cast<size_t>(index * kEventWords);
        TlTerminalEvent event{};
        event.abi_version = TL_TERMINAL_ABI_VERSION;
        event.struct_size = sizeof(TlTerminalEvent);
        event.source_sequence = static_cast<uint64_t>(words[offset]);
        event.kind = static_cast<uint32_t>(words[offset + 1]);
        event.columns = static_cast<uint32_t>(words[offset + 2]);
        event.rows = static_cast<uint32_t>(words[offset + 3]);
        event.advance_micros = static_cast<uint64_t>(words[offset + 4]);
        if (event.kind == TL_EVENT_OUTPUT || event.kind == TL_EVENT_HOST_GEOMETRY_V1 || event.kind == TL_EVENT_HOST_THEME_V1) {
            auto payload = static_cast<jbyteArray>(env->GetObjectArrayElement(raw_payloads, index));
            if (payload == nullptr || env->ExceptionCheck()) {
                return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_INVALID_ARGUMENT;
            }
            auto pin = std::make_unique<ByteArrayPin>(env, payload, true);
            if (!pin->valid()) {
                return TL_STATUS_PANIC;
            }
            event.data = pin->data();
            event.data_len = pin->size();
            pins->push_back(std::move(pin));
        }
        events->push_back(event);
    }
    return TL_STATUS_OK;
}

uint32_t checkpoint_for(TlTerminal* terminal, CheckpointOwner* checkpoint,
                        TlTerminalCheckpointInfo* info) {
    uint32_t status = tl_terminal_checkpoint_begin(terminal, checkpoint->out());
    if (status != TL_STATUS_OK) {
        return status;
    }
    if (info != nullptr) {
        *info = TlTerminalCheckpointInfo{TL_TERMINAL_ABI_VERSION,
                                         sizeof(TlTerminalCheckpointInfo), 0, 0, {0, 0}};
        status = tl_terminal_checkpoint_get_info(checkpoint->get(), info);
    }
    return status;
}

}  // namespace

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_abiVersion(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_abi_version());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV1FormatVersion(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_render_frame_v1_format_version());
}
extern "C" JNIEXPORT jlong JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV1MaxBytes(JNIEnv*, jobject) {
    return static_cast<jlong>(tl_terminal_render_frame_v1_max_bytes());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV3FormatVersion(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_render_frame_v3_format_version());
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV4FormatVersion(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_render_frame_v4_format_version());
}
extern "C" JNIEXPORT jlong JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV3MaxBytes(JNIEnv*, jobject) {
    return static_cast<jlong>(tl_terminal_render_frame_v3_max_bytes());
}

extern "C" JNIEXPORT jlong JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV4MaxBytes(JNIEnv*, jobject) {
    return static_cast<jlong>(tl_terminal_render_frame_v4_max_bytes());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV5FormatVersion(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_render_frame_v5_format_version());
}
extern "C" JNIEXPORT jlong JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV5MaxBytes(JNIEnv*, jobject) {
    return static_cast<jlong>(tl_terminal_render_frame_v5_max_bytes());
}
extern "C" JNIEXPORT jlong JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_maxGraphicsSnapshotBytes(JNIEnv*, jobject) {
    return static_cast<jlong>(tl_terminal_max_graphics_snapshot_bytes());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_hostEffectFrameV2FormatVersion(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_host_effect_frame_v2_format_version());
}
extern "C" JNIEXPORT jlong JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_hostEffectFrameV2MaxBytes(JNIEnv*, jobject) {
    return static_cast<jlong>(tl_terminal_host_effect_frame_v2_max_bytes());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_mouseInputV1StructSize(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_mouse_input_v1_struct_size());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_inputV1StructSize(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_input_v1_struct_size());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_inputV2StructSize(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_input_v2_struct_size());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_inputV3StructSize(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_input_v3_struct_size());
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_configV3StructSize(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_config_v3_struct_size());
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_configV4StructSize(JNIEnv*, jobject) {
    return static_cast<jint>(tl_terminal_config_v4_struct_size());
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_create(
    JNIEnv* env, jobject, jint columns, jint rows, jlong initial_sequence, jlongArray output) {
    return safe_status([&]() -> uint32_t {
        if (!minimum_length(env, output, kAllocationWords) || initial_sequence < 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalConfigV1 config{TL_TERMINAL_ABI_VERSION, sizeof(TlTerminalConfigV1),
                                  static_cast<uint32_t>(columns), static_cast<uint32_t>(rows),
                                  static_cast<uint64_t>(initial_sequence), {0, 0}};
        TlTerminal* terminal = nullptr;
        const uint32_t status = tl_terminal_create(&config, &terminal);
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_allocation(env, output, terminal, static_cast<uint64_t>(initial_sequence));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_createV3(
    JNIEnv* env, jobject, jlongArray raw_config, jlongArray output) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, raw_config, kConfigV3Words, &words) ||
            !minimum_length(env, output, kAllocationWords)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        if (words[4] < 0 || words[7] < 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalConfigV3 config{};
        config.abi_version = static_cast<uint32_t>(words[0]);
        config.struct_size = static_cast<uint32_t>(words[1]);
        config.columns = static_cast<uint32_t>(words[2]);
        config.rows = static_cast<uint32_t>(words[3]);
        config.initial_source_sequence = static_cast<uint64_t>(words[4]);
        config.reserved_v1_compat[0] = static_cast<uint64_t>(words[5]);
        config.reserved_v1_compat[1] = static_cast<uint64_t>(words[6]);
        config.history_capacity_rows = static_cast<uint64_t>(words[7]);
        config.reserved_v2_compat[0] = static_cast<uint64_t>(words[8]);
        config.reserved_v2_compat[1] = static_cast<uint64_t>(words[9]);
        config.output_profile = static_cast<uint32_t>(words[10]);
        config.reserved0 = static_cast<uint32_t>(words[11]);
        config.reserved[0] = static_cast<uint64_t>(words[12]);
        config.reserved[1] = static_cast<uint64_t>(words[13]);
        TlTerminal* terminal = nullptr;
        const uint32_t status = tl_terminal_create_v3(&config, &terminal);
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_allocation(env, output, terminal, config.initial_source_sequence);
    });
}

static jint create_with_v4_config(
    JNIEnv* env, jlongArray raw_config, jlongArray output, bool graphics) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, raw_config, kConfigV4Words, &words) ||
            !minimum_length(env, output, kAllocationWords)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        if (words[4] < 0 || words[7] < 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        for (const size_t index : {0u, 1u, 2u, 3u, 10u, 11u, 14u, 15u}) {
            if (words[index] < 0 || static_cast<uint64_t>(words[index]) > UINT32_MAX) {
                return TL_STATUS_INVALID_ARGUMENT;
            }
        }
        TlTerminalConfigV4 config{};
        config.abi_version = static_cast<uint32_t>(words[0]);
        config.struct_size = static_cast<uint32_t>(words[1]);
        config.columns = static_cast<uint32_t>(words[2]);
        config.rows = static_cast<uint32_t>(words[3]);
        config.initial_source_sequence = static_cast<uint64_t>(words[4]);
        config.reserved_v1_compat[0] = static_cast<uint64_t>(words[5]);
        config.reserved_v1_compat[1] = static_cast<uint64_t>(words[6]);
        config.history_capacity_rows = static_cast<uint64_t>(words[7]);
        config.reserved_v2_compat[0] = static_cast<uint64_t>(words[8]);
        config.reserved_v2_compat[1] = static_cast<uint64_t>(words[9]);
        config.output_profile = static_cast<uint32_t>(words[10]);
        config.reserved0 = static_cast<uint32_t>(words[11]);
        config.reserved_v3_compat[0] = static_cast<uint64_t>(words[12]);
        config.reserved_v3_compat[1] = static_cast<uint64_t>(words[13]);
        config.width_profile = static_cast<uint32_t>(words[14]);
        config.reserved1 = static_cast<uint32_t>(words[15]);
        config.reserved[0] = static_cast<uint64_t>(words[16]);
        config.reserved[1] = static_cast<uint64_t>(words[17]);
        TlTerminal* terminal = nullptr;
        const uint32_t status = graphics ? tl_terminal_create_graphics_v1(&config, &terminal)
                                         : tl_terminal_create_v4(&config, &terminal);
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_allocation(env, output, terminal, config.initial_source_sequence);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_createV4(
    JNIEnv* env, jobject, jlongArray raw_config, jlongArray output) {
    return create_with_v4_config(env, raw_config, output, false);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_createGraphicsV1(
    JNIEnv* env, jobject, jlongArray raw_config, jlongArray output) {
    return create_with_v4_config(env, raw_config, output, true);
}

static jint import_snapshot_profile(
    JNIEnv* env, jbyteArray snapshot, jlongArray output, bool graphics) {
    return safe_status([&]() -> uint32_t {
        if (!minimum_length(env, output, kAllocationWords)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        const uint64_t maximum = graphics ? TL_GRAPHICS_SNAPSHOT_MAX_BYTES : kMaximumSnapshotBytes;
        if (snapshot == nullptr || env->GetArrayLength(snapshot) == 0 ||
            static_cast<uint64_t>(env->GetArrayLength(snapshot)) > maximum) {
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        ByteArrayPin pin(env, snapshot);
        if (!pin.valid()) {
            return TL_STATUS_PANIC;
        }
        if (pin.size() == 0 || pin.size() > maximum) {
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        TlTerminal* terminal = nullptr;
        uint32_t status = graphics ? tl_terminal_import_snapshot_graphics_v1(pin.data(), pin.size(), &terminal)
                                   : tl_terminal_import_snapshot(pin.data(), pin.size(), &terminal);
        if (status != TL_STATUS_OK) {
            return status;
        }
        TlTerminalPresentationInfoV1 info{TL_TERMINAL_ABI_VERSION,
                                          sizeof(TlTerminalPresentationInfoV1), 0, 0, 0, 0, 0, {0}};
        status = tl_terminal_get_presentation_info_v1(terminal, &info);
        if (status != TL_STATUS_OK) {
            tl_terminal_destroy(terminal);
            return status;
        }
        if (graphics && info.next_source_sequence > INT64_MAX) {
            tl_terminal_destroy(terminal);
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        return publish_allocation(env, output, terminal, info.next_source_sequence);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_importCheckpoint(
    JNIEnv* env, jobject, jbyteArray snapshot, jlongArray output) {
    return import_snapshot_profile(env, snapshot, output, false);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_importSnapshotGraphicsV1(
    JNIEnv* env, jobject, jbyteArray snapshot, jlongArray output) {
    return import_snapshot_profile(env, snapshot, output, true);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_destroy(JNIEnv*, jobject, jlong token) {
    return safe_status([&]() { return destroy_token(token); });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBatch(
    JNIEnv* env, jobject, jlong token, jlongArray raw_events, jobjectArray raw_payloads) {
    return safe_status([&]() -> uint32_t {
        std::vector<TlTerminalEvent> events;
        std::vector<std::unique_ptr<ByteArrayPin>> pins;
        const uint32_t marshal_status = marshal_events(env, raw_events, raw_payloads, &events, &pins);
        if (marshal_status != TL_STATUS_OK) {
            return marshal_status;
        }
        return with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_apply_event_batch_v1(terminal, events.data(),
                                                     static_cast<uint32_t>(events.size()));
        });
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBatchWithHostEffectsV2(
    JNIEnv* env, jobject, jlong token, jlongArray raw_events, jobjectArray raw_payloads,
    jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        // The Rust call commits atomically, so reject an unusable Java output slot first.
        if (!minimum_length(env, output, 1)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        std::vector<TlTerminalEvent> events;
        std::vector<std::unique_ptr<ByteArrayPin>> pins;
        const uint32_t marshal_status = marshal_events(env, raw_events, raw_payloads, &events, &pins);
        if (marshal_status != TL_STATUS_OK) {
            return marshal_status;
        }
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_apply_event_batch_with_host_effects_v2(
                terminal, events.data(), static_cast<uint32_t>(events.size()), buffer.out());
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_buffer(env, output, &buffer, tl_terminal_host_effect_frame_v2_max_bytes());
    });
}

// Android never receives the broker reference or opens a Windows-side file.
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBrokerUnsupportedV1(
    JNIEnv* env, jobject, jlong token, jlong source_sequence, jbyteArray raw_output,
    jobjectArray output_effects, jobjectArray output_diagnostic) {
    return safe_status([&]() -> uint32_t {
        if (source_sequence < 0 || raw_output == nullptr ||
            env->GetArrayLength(raw_output) > 1024 * 1024 ||
            !minimum_length(env, output_effects, 1) ||
            !minimum_length(env, output_diagnostic, 1)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        jobject prior = env->GetObjectArrayElement(output_effects, 0);
        if (env->ExceptionCheck()) return TL_STATUS_PANIC;
        if (prior != nullptr) {
            env->DeleteLocalRef(prior);
            return TL_STATUS_INVALID_ARGUMENT;
        }
        auto entry = find_terminal(token);
        if (entry == nullptr) return TL_STATUS_INVALID_ARGUMENT;
        std::lock_guard lock(entry->mutex);
        if (entry->terminal == nullptr) return TL_STATUS_INVALID_ARGUMENT;
        ByteArrayPin bytes(env, raw_output);
        if (!bytes.valid()) return TL_STATUS_PANIC;
        TlTerminalEvent event{};
        event.abi_version = TL_TERMINAL_ABI_VERSION;
        event.struct_size = sizeof(event);
        event.source_sequence = static_cast<uint64_t>(source_sequence);
        event.kind = TL_EVENT_OUTPUT;
        event.data = bytes.data();
        event.data_len = bytes.size();
        BufferOwner diagnostic;
        BrokerOwner broker(&entry->terminal);
        uint32_t status = tl_terminal_broker_begin_v1(
            &entry->terminal, &event, broker.out(), diagnostic.out());
        if (status != TL_STATUS_OK) {
            if (diagnostic.get().data != nullptr) {
                (void)publish_buffer(env, output_diagnostic, &diagnostic, 8192);
            }
            return status;
        }
        for (unsigned index = 0; index < 64; ++index) {
            TlTerminalBrokerRequestV1 request{};
            request.abi_version = TL_TERMINAL_ABI_VERSION;
            request.struct_size = sizeof(request);
            status = tl_terminal_broker_request_redacted_v1(broker.get(), &request);
            if (status != TL_STATUS_OK) return status;
            if (request.has_request == 0) break;
            status = tl_terminal_broker_supply_v1(
                broker.get(), &request, TL_BROKER_CAPTURE_UNSUPPORTED, nullptr, 0);
            if (status != TL_STATUS_OK) {
                (void)tl_terminal_broker_diagnostic_v1(broker.get(), diagnostic.out());
                if (diagnostic.get().data != nullptr) {
                    (void)publish_buffer(env, output_diagnostic, &diagnostic, 8192);
                }
                return status;
            }
        }
        BufferOwner preview;
        status = tl_terminal_broker_preview_v1(broker.get(), preview.out());
        if (status != TL_STATUS_OK) return status;
        status = publish_buffer(env, output_effects, &preview,
                                tl_terminal_host_effect_frame_v2_max_bytes());
        if (status != TL_STATUS_OK) return status;
        BufferOwner committed;
        status = broker.commit(committed.out());
        if (status != TL_STATUS_OK) {
            env->SetObjectArrayElement(output_effects, 0, nullptr);
        }
        return env->ExceptionCheck() ? TL_STATUS_PANIC : status;
    });
}

// Identities are the 96-byte redacted TlTerminalBrokerRequestV1 fields in little-endian order.
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBrokerCapturedV1(
    JNIEnv* env, jobject, jlong token, jlong source_sequence, jbyteArray raw_output,
    jobjectArray raw_identities, jintArray raw_outcomes, jobjectArray raw_captures,
    jobjectArray output_effects, jobjectArray output_diagnostic) {
    return safe_status([&]() -> uint32_t {
        if (source_sequence < 0 || raw_output == nullptr ||
            env->GetArrayLength(raw_output) > 1024 * 1024 ||
            raw_identities == nullptr || raw_outcomes == nullptr || raw_captures == nullptr ||
            !minimum_length(env, output_effects, 1) ||
            !minimum_length(env, output_diagnostic, 1)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        const jsize count = env->GetArrayLength(raw_identities);
        if (count > kMaximumBrokerCaptures) return TL_STATUS_LIMIT_EXCEEDED;
        if (env->GetArrayLength(raw_outcomes) != count ||
            env->GetArrayLength(raw_captures) != count) return TL_STATUS_INVALID_ARGUMENT;
        jobject prior_effects = env->GetObjectArrayElement(output_effects, 0);
        if (env->ExceptionCheck()) return TL_STATUS_PANIC;
        jobject prior_diagnostic = env->GetObjectArrayElement(output_diagnostic, 0);
        if (env->ExceptionCheck()) {
            if (prior_effects != nullptr) env->DeleteLocalRef(prior_effects);
            return TL_STATUS_PANIC;
        }
        const bool occupied = prior_effects != nullptr || prior_diagnostic != nullptr;
        if (prior_effects != nullptr) env->DeleteLocalRef(prior_effects);
        if (prior_diagnostic != nullptr) env->DeleteLocalRef(prior_diagnostic);
        if (occupied) return TL_STATUS_INVALID_ARGUMENT;

        std::vector<jint> outcomes(static_cast<size_t>(count));
        if (count != 0) env->GetIntArrayRegion(raw_outcomes, 0, count, outcomes.data());
        if (env->ExceptionCheck()) return TL_STATUS_PANIC;
        std::vector<jsize> lengths(static_cast<size_t>(count));
        uint64_t total_bytes = 0;
        for (jsize index = 0; index < count; ++index) {
            auto identity = static_cast<jbyteArray>(env->GetObjectArrayElement(raw_identities, index));
            if (identity == nullptr || env->ExceptionCheck()) {
                if (identity != nullptr) env->DeleteLocalRef(identity);
                return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_INVALID_ARGUMENT;
            }
            const bool identity_size_ok = env->GetArrayLength(identity) == kBrokerIdentityBytes;
            env->DeleteLocalRef(identity);
            if (!identity_size_ok) return TL_STATUS_INVALID_ARGUMENT;
            auto capture = static_cast<jbyteArray>(env->GetObjectArrayElement(raw_captures, index));
            if (capture == nullptr || env->ExceptionCheck()) {
                if (capture != nullptr) env->DeleteLocalRef(capture);
                return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_INVALID_ARGUMENT;
            }
            const jsize length = env->GetArrayLength(capture);
            env->DeleteLocalRef(capture);
            const jint outcome = outcomes[static_cast<size_t>(index)];
            if (outcome < 0 || static_cast<uint32_t>(outcome) > TL_BROKER_CAPTURE_OVER_LIMIT ||
                (outcome != 0 && length != 0)) {
                return TL_STATUS_INVALID_ARGUMENT;
            }
            const uint64_t size = static_cast<uint64_t>(length);
            if (size > kMaximumBrokerCaptureBytes || size > kMaximumBrokerOutputBytes - total_bytes) {
                return TL_STATUS_LIMIT_EXCEEDED;
            }
            total_bytes += size;
            lengths[static_cast<size_t>(index)] = length;
        }

        // No captured bytes are copied until the whole batch passes the size budget.
        std::vector<TlTerminalBrokerRequestV1> identities(static_cast<size_t>(count));
        std::vector<std::vector<uint8_t>> captures(static_cast<size_t>(count));
        for (jsize index = 0; index < count; ++index) {
            auto identity = static_cast<jbyteArray>(env->GetObjectArrayElement(raw_identities, index));
            if (identity == nullptr || env->ExceptionCheck()) {
                if (identity != nullptr) env->DeleteLocalRef(identity);
                return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_INVALID_ARGUMENT;
            }
            if (env->GetArrayLength(identity) != kBrokerIdentityBytes) {
                env->DeleteLocalRef(identity);
                return TL_STATUS_INVALID_ARGUMENT;
            }
            uint8_t words[kBrokerIdentityBytes]{};
            env->GetByteArrayRegion(identity, 0, kBrokerIdentityBytes,
                                    reinterpret_cast<jbyte*>(words));
            env->DeleteLocalRef(identity);
            if (env->ExceptionCheck()) return TL_STATUS_PANIC;
            if (!decode_broker_identity(words, &identities[static_cast<size_t>(index)])) {
                return TL_STATUS_INVALID_ARGUMENT;
            }
            auto capture = static_cast<jbyteArray>(env->GetObjectArrayElement(raw_captures, index));
            if (capture == nullptr || env->ExceptionCheck()) {
                if (capture != nullptr) env->DeleteLocalRef(capture);
                return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_INVALID_ARGUMENT;
            }
            const jsize length = lengths[static_cast<size_t>(index)];
            if (env->GetArrayLength(capture) != length) {
                env->DeleteLocalRef(capture);
                return TL_STATUS_INVALID_ARGUMENT;
            }
            auto& copied = captures[static_cast<size_t>(index)];
            copied.resize(static_cast<size_t>(length));
            if (length != 0) {
                env->GetByteArrayRegion(capture, 0, length,
                                        reinterpret_cast<jbyte*>(copied.data()));
            }
            env->DeleteLocalRef(capture);
            if (env->ExceptionCheck()) return TL_STATUS_PANIC;
        }

        auto entry = find_terminal(token);
        if (entry == nullptr) return TL_STATUS_INVALID_ARGUMENT;
        std::lock_guard lock(entry->mutex);
        if (entry->terminal == nullptr) return TL_STATUS_INVALID_ARGUMENT;
        ByteArrayPin bytes(env, raw_output);
        if (!bytes.valid()) return TL_STATUS_PANIC;
        TlTerminalEvent event{};
        event.abi_version = TL_TERMINAL_ABI_VERSION;
        event.struct_size = sizeof(event);
        event.source_sequence = static_cast<uint64_t>(source_sequence);
        event.kind = TL_EVENT_OUTPUT;
        event.data = bytes.data();
        event.data_len = bytes.size();
        BufferOwner diagnostic;
        BrokerOwner broker(&entry->terminal);
        uint32_t status = tl_terminal_broker_begin_v1(
            &entry->terminal, &event, broker.out(), diagnostic.out());
        if (status != TL_STATUS_OK) {
            if (diagnostic.get().data != nullptr) {
                (void)publish_buffer(env, output_diagnostic, &diagnostic, 8192);
            }
            return status;
        }
        for (jsize index = 0; index < count; ++index) {
            TlTerminalBrokerRequestV1 request{};
            request.abi_version = TL_TERMINAL_ABI_VERSION;
            request.struct_size = sizeof(request);
            status = tl_terminal_broker_request_redacted_v1(broker.get(), &request);
            if (status != TL_STATUS_OK) return status;
            if (request.has_request != 1 ||
                !same_broker_identity(request, identities[static_cast<size_t>(index)])) {
                return TL_STATUS_INVALID_ARGUMENT;
            }
            const auto& capture = captures[static_cast<size_t>(index)];
            status = tl_terminal_broker_supply_v1(
                broker.get(), &request, static_cast<uint32_t>(outcomes[static_cast<size_t>(index)]),
                capture.empty() ? nullptr : capture.data(), capture.size());
            if (status != TL_STATUS_OK) {
                (void)tl_terminal_broker_diagnostic_v1(broker.get(), diagnostic.out());
                if (diagnostic.get().data != nullptr) {
                    (void)publish_buffer(env, output_diagnostic, &diagnostic, 8192);
                }
                return status;
            }
        }
        TlTerminalBrokerRequestV1 next{};
        next.abi_version = TL_TERMINAL_ABI_VERSION;
        next.struct_size = sizeof(next);
        status = tl_terminal_broker_request_redacted_v1(broker.get(), &next);
        if (status != TL_STATUS_OK) return status;
        if (next.has_request != 0) return TL_STATUS_INVALID_ARGUMENT;
        BufferOwner preview;
        status = tl_terminal_broker_preview_v1(broker.get(), preview.out());
        if (status != TL_STATUS_OK) return status;
        status = publish_buffer(env, output_effects, &preview,
                                tl_terminal_host_effect_frame_v2_max_bytes());
        if (status != TL_STATUS_OK) return status;
        BufferOwner committed;
        status = broker.commit(committed.out());
        if (status != TL_STATUS_OK) env->SetObjectArrayElement(output_effects, 0, nullptr);
        return env->ExceptionCheck() ? TL_STATUS_PANIC : status;
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_render(
    JNIEnv* env, jobject, jlong token, jlong maximum, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (maximum <= 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_export_render_frame_v1(
                    checkpoint.get(), static_cast<uint64_t>(maximum), buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_buffer(env, output, &buffer, static_cast<uint64_t>(maximum));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_failureDiagnosticV3(
    JNIEnv* env, jobject, jlong token, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (!minimum_length(env, output, 1)) return TL_STATUS_INVALID_ARGUMENT;
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_export_failure_diagnostic_v3(terminal, buffer.out());
        });
        if (status != TL_STATUS_OK) return status;
        if (buffer.get().len == 0 && buffer.get().data == nullptr) {
            env->SetObjectArrayElement(output, 0, nullptr);
            return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_OK;
        }
        return publish_buffer(env, output, &buffer, 8192);
    });
}
extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_failureDiagnosticV2(
    JNIEnv* env, jobject, jlong token, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (!minimum_length(env, output, 1)) return TL_STATUS_INVALID_ARGUMENT;
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_export_failure_diagnostic_v2(terminal, buffer.out());
        });
        if (status != TL_STATUS_OK) return status;
        if (buffer.get().len == 0 && buffer.get().data == nullptr) {
            env->SetObjectArrayElement(output, 0, nullptr);
            return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_OK;
        }
        return publish_buffer(env, output, &buffer, 4096);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_failureDiagnosticV1(
    JNIEnv* env, jobject, jlong token, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (!minimum_length(env, output, 1)) return TL_STATUS_INVALID_ARGUMENT;
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_export_failure_diagnostic_v1(terminal, buffer.out());
        });
        if (status != TL_STATUS_OK) return status;
        if (buffer.get().len == 0 && buffer.get().data == nullptr) {
            env->SetObjectArrayElement(output, 0, nullptr);
            return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_OK;
        }
        return publish_buffer(env, output, &buffer, 4096);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV3(
    JNIEnv* env, jobject, jlong token, jlong maximum, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (maximum <= 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_export_render_frame_v3(
                    checkpoint.get(), static_cast<uint64_t>(maximum), buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_buffer(env, output, &buffer, static_cast<uint64_t>(maximum));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_hostSignalsV1(
    JNIEnv* env, jobject, jlong token, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_get_host_signals_v1(terminal, buffer.out());
        });
        if (status != TL_STATUS_OK) return status;
        return publish_buffer(env, output, &buffer, 12);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_shellStateV1(
    JNIEnv* env, jobject, jlong token, jlong maximum, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (maximum <= 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_get_shell_state_v1(
                terminal, static_cast<uint64_t>(maximum), buffer.out());
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_buffer(env, output, &buffer, static_cast<uint64_t>(maximum));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV4(
    JNIEnv* env, jobject, jlong token, jlong maximum, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (maximum <= 0) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_export_render_frame_v4(
                    checkpoint.get(), static_cast<uint64_t>(maximum), buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        return publish_buffer(env, output, &buffer, static_cast<uint64_t>(maximum));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV3AtScrollback(
    JNIEnv* env, jobject, jlong token, jlong offset_rows, jlong maximum,
    jlongArray output_info, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (offset_rows < 0 || maximum <= 0 || !minimum_length(env, output_info, 3) ||
            !minimum_length(env, output, 1) ||
            static_cast<uint64_t>(offset_rows) > std::numeric_limits<uint32_t>::max()) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        uint32_t available_rows = 0;
        uint64_t generation_rows = 0;
        uint32_t applied_offset = 0;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_get_scrollback_info_v1(
                    checkpoint.get(), &available_rows, &generation_rows);
            }
            if (inner == TL_STATUS_OK) {
                applied_offset = std::min(static_cast<uint32_t>(offset_rows), available_rows);
                inner = tl_terminal_checkpoint_export_render_frame_v3_at_scrollback(
                    checkpoint.get(), applied_offset, static_cast<uint64_t>(maximum),
                    buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        if (generation_rows > static_cast<uint64_t>(std::numeric_limits<jlong>::max())) {
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        const std::vector<jlong> values{
            static_cast<jlong>(available_rows), static_cast<jlong>(generation_rows),
            static_cast<jlong>(applied_offset)};
        if (!write_longs(env, output_info, values)) {
            return TL_STATUS_PANIC;
        }
        return publish_buffer(env, output, &buffer, static_cast<uint64_t>(maximum));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyBegin(
    JNIEnv* env, jobject, jlong token, jlongArray output) {
    return safe_status([&]() -> uint32_t {
        if (!minimum_length(env, output, 5)) return TL_STATUS_INVALID_ARGUMENT;
        TlTerminalHistoryCaptureV1 capture{};
        capture.abi_version = TL_TERMINAL_ABI_VERSION;
        capture.struct_size = sizeof(capture);
        // Allocate the Java publication buffer before taking ownership of a capture.
        std::vector<jlong> values(5);
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return tl_terminal_history_begin_v1(terminal, &capture);
        });
        if (status != TL_STATUS_OK) return status;
        if (capture.capture_id > INT64_MAX || capture.owner_id > INT64_MAX || capture.revision > INT64_MAX) {
            (void)tl_terminal_history_release_v1(capture.capture_id, capture.owner_id);
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        values[0] = static_cast<jlong>(capture.capture_id);
        values[1] = static_cast<jlong>(capture.owner_id);
        values[2] = static_cast<jlong>(capture.revision);
        values[3] = static_cast<jlong>(capture.accounted_bytes);
        values[4] = static_cast<jlong>(capture.lease_millis);
        if (!write_longs(env, output, values)) {
            (void)tl_terminal_history_release_v1(capture.capture_id, capture.owner_id);
            return TL_STATUS_PANIC;
        }
        return TL_STATUS_OK;
    });
}

static jint history_page(
    JNIEnv* env, jlongArray input, jlongArray required, jobjectArray output, bool graphics) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, input, 7, &words) || !minimum_length(env, required, 2) ||
            !minimum_length(env, output, 1) || words[0] <= 0 || words[1] <= 0 || words[2] < 0 ||
            words[3] < -1 || words[4] < 1 || words[4] > 128 || words[5] < 1 || words[5] > 65536 ||
            words[6] < 1 || words[6] > 4 * 1024 * 1024) return TL_STATUS_INVALID_ARGUMENT;
        TlTerminalHistoryRequestV1 query{};
        query.abi_version = TL_TERMINAL_ABI_VERSION;
        query.struct_size = sizeof(query);
        query.capture_id = static_cast<uint64_t>(words[0]);
        query.owner_id = static_cast<uint64_t>(words[1]);
        query.revision = static_cast<uint64_t>(words[2]);
        query.before_line_id = static_cast<uint64_t>(words[3]);
        query.max_lines = static_cast<uint32_t>(words[4]);
        query.max_cells = static_cast<uint32_t>(words[5]);
        query.max_utf8_bytes = static_cast<uint32_t>(words[6]);
        BufferOwner buffer;
        uint32_t cells = 0, utf8 = 0;
        const uint32_t status = graphics ? tl_terminal_history_page_v2(&query, buffer.out(), &cells, &utf8)
                                         : tl_terminal_history_page_v1(&query, buffer.out(), &cells, &utf8);
        if (!write_longs(env, required, {static_cast<jlong>(cells), static_cast<jlong>(utf8)})) return TL_STATUS_PANIC;
        if (status != TL_STATUS_OK) return status;
        return publish_buffer(env, output, &buffer, graphics ? TL_HISTORY_PAGE_V2_MAX_BYTES : TL_HISTORY_PAGE_V1_MAX_BYTES);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV5(
    JNIEnv* env, jobject, jlong token, jint flags, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (flags != 0 || !minimum_length(env, output, 1)) return TL_STATUS_INVALID_ARGUMENT;
        BufferOwner buffer;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_export_render_frame_v5(checkpoint.get(), 0, buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) return status;
        return publish_buffer(env, output, &buffer, TL_RENDER_FRAME_V5_MAX_BYTES);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV5AtScrollback(
    JNIEnv* env, jobject, jlong token, jlong offset_rows, jint flags,
    jlongArray output_info, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (offset_rows < 0 || static_cast<uint64_t>(offset_rows) > UINT32_MAX || flags != 0 ||
            !minimum_length(env, output_info, 3) || !minimum_length(env, output, 1)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        uint32_t rows = 0;
        uint64_t generation = 0;
        uint32_t applied_offset = 0;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_get_scrollback_info_v1(checkpoint.get(), &rows, &generation);
            }
            if (inner == TL_STATUS_OK) {
                applied_offset = std::min(static_cast<uint32_t>(offset_rows), rows);
                inner = tl_terminal_checkpoint_export_render_frame_v5_at_scrollback(
                    checkpoint.get(), applied_offset, 0, buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) return status;
        if (generation > INT64_MAX) return TL_STATUS_LIMIT_EXCEEDED;
        if (!write_longs(env, output_info, {static_cast<jlong>(rows), static_cast<jlong>(generation),
                                         static_cast<jlong>(applied_offset)})) return TL_STATUS_PANIC;
        return publish_buffer(env, output, &buffer, TL_RENDER_FRAME_V5_MAX_BYTES);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyPage(
    JNIEnv* env, jobject, jlongArray input, jlongArray required, jobjectArray output) {
    return history_page(env, input, required, output, false);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyPageV2(
    JNIEnv* env, jobject, jlongArray input, jlongArray required, jobjectArray output) {
    return history_page(env, input, required, output, true);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyRelease(
    JNIEnv*, jobject, jlong capture, jlong owner) {
    return safe_status([&]() -> uint32_t {
        return tl_terminal_history_release_v1(static_cast<uint64_t>(capture), static_cast<uint64_t>(owner));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyResolveAnchor(
    JNIEnv* env, jobject, jlong capture, jlong owner, jlong revision, jlong line, jlong cell, jlongArray output) {
    return safe_status([&]() -> uint32_t {
        if (cell < 0 || static_cast<uint64_t>(cell) > UINT32_MAX || !minimum_length(env,output,1)) return TL_STATUS_INVALID_ARGUMENT;
        uint32_t offset = 0;
        const uint32_t status = tl_terminal_history_resolve_anchor_v1(static_cast<uint64_t>(capture),
            static_cast<uint64_t>(owner),static_cast<uint64_t>(revision),static_cast<uint64_t>(line),static_cast<uint32_t>(cell),&offset);
        if (status != TL_STATUS_OK) return status;
        return write_longs(env,output,{static_cast<jlong>(offset)}) ? TL_STATUS_OK : TL_STATUS_PANIC;
    });
}

static jint history_viewport(
    JNIEnv* env, jlong capture, jlong owner, jlong revision, jlong offset,
    jlongArray info, jobjectArray output, bool graphics) {
    return safe_status([&]() -> uint32_t {
        if (offset < 0 || static_cast<uint64_t>(offset) > UINT32_MAX || !minimum_length(env,info,3) ||
            !minimum_length(env,output,1)) return TL_STATUS_INVALID_ARGUMENT;
        BufferOwner buffer;
        uint32_t rows = 0; uint64_t generation = 0;
        const auto project = graphics ? tl_terminal_history_viewport_v2 : tl_terminal_history_viewport_v1;
        const uint32_t result = project(static_cast<uint64_t>(capture),
            static_cast<uint64_t>(owner), static_cast<uint64_t>(revision), static_cast<uint32_t>(offset), buffer.out(), &rows, &generation);
        if (result != TL_STATUS_OK) return result;
        if (generation > INT64_MAX) return TL_STATUS_LIMIT_EXCEEDED;
        if (!write_longs(env,info,{static_cast<jlong>(rows),static_cast<jlong>(generation),std::min(offset,static_cast<jlong>(rows))})) return TL_STATUS_PANIC;
        return publish_buffer(env,output,&buffer,graphics ? TL_RENDER_FRAME_V5_MAX_BYTES : TL_RENDER_FRAME_V4_MAX_BYTES);
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyViewport(
    JNIEnv* env, jobject, jlong capture, jlong owner, jlong revision, jlong offset,
    jlongArray info, jobjectArray output) {
    return history_viewport(env, capture, owner, revision, offset, info, output, false);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyViewportV2(
    JNIEnv* env, jobject, jlong capture, jlong owner, jlong revision, jlong offset,
    jlongArray info, jobjectArray output) {
    return history_viewport(env, capture, owner, revision, offset, info, output, true);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV4AtScrollback(
    JNIEnv* env, jobject, jlong token, jlong offset_rows, jlong maximum,
    jlongArray output_info, jobjectArray output) {
    return safe_status([&]() -> uint32_t {
        if (offset_rows < 0 || maximum <= 0 || !minimum_length(env, output_info, 3) ||
            !minimum_length(env, output, 1) ||
            static_cast<uint64_t>(offset_rows) > std::numeric_limits<uint32_t>::max()) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        uint32_t available_rows = 0;
        uint64_t generation_rows = 0;
        uint32_t applied_offset = 0;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, nullptr);
            if (inner == TL_STATUS_OK) {
                inner = tl_terminal_checkpoint_get_scrollback_info_v1(
                    checkpoint.get(), &available_rows, &generation_rows);
            }
            if (inner == TL_STATUS_OK) {
                applied_offset = std::min(static_cast<uint32_t>(offset_rows), available_rows);
                inner = tl_terminal_checkpoint_export_render_frame_v4_at_scrollback(
                    checkpoint.get(), applied_offset, static_cast<uint64_t>(maximum),
                    buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        if (generation_rows > static_cast<uint64_t>(std::numeric_limits<jlong>::max())) {
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        const std::vector<jlong> values{
            static_cast<jlong>(available_rows), static_cast<jlong>(generation_rows),
            static_cast<jlong>(applied_offset)};
        if (!write_longs(env, output_info, values)) {
            return TL_STATUS_PANIC;
        }
        return publish_buffer(env, output, &buffer, static_cast<uint64_t>(maximum));
    });
}

static jint encode_input_v1_jni(
    JNIEnv* env, jlong token, jlongArray raw_input, jbyteArray raw_data,
    jobjectArray output, jobjectArray diagnostic) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, raw_input, kInputV1Words, &words)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        ByteArrayPin data(env, raw_data);
        if (!data.valid()) {
            return TL_STATUS_PANIC;
        }
        if (words[6] < 0 || static_cast<uint64_t>(words[6]) != data.size()) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalInputV1 input{};
        input.abi_version = static_cast<uint32_t>(words[0]);
        input.struct_size = static_cast<uint32_t>(words[1]);
        input.kind = static_cast<uint32_t>(words[2]);
        input.key = static_cast<uint32_t>(words[3]);
        input.modifiers = static_cast<uint32_t>(words[4]);
        input.character = static_cast<uint32_t>(words[5]);
        input.data = data.data();
        input.data_len = data.size();
        input.reserved[0] = static_cast<uint64_t>(words[7]);
        input.reserved[1] = static_cast<uint64_t>(words[8]);
        if (diagnostic != nullptr &&
            (!minimum_length(env, diagnostic, 1) || env->IsSameObject(output, diagnostic))) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        BufferOwner detail;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return diagnostic == nullptr
                ? tl_terminal_encode_input_v1(terminal, &input, buffer.out())
                : tl_terminal_encode_input_v1_with_diagnostic(
                    terminal, &input, buffer.out(), detail.out());
        });
        if (status != TL_STATUS_OK) {
            if (diagnostic != nullptr && detail.get().data != nullptr) {
                const uint32_t publication = publish_buffer(
                    env, diagnostic, &detail, TL_INPUT_DIAGNOSTIC_V1_MAX_BYTES);
                if (publication != TL_STATUS_OK) {
                    return publication;
                }
            }
            return status;
        }
        return publish_buffer(env, output, &buffer, tl_terminal_host_effect_frame_v2_max_bytes());
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV1(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jbyteArray raw_data,
    jobjectArray output) {
    return encode_input_v1_jni(env, token, raw_input, raw_data, output, nullptr);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV1WithDiagnostic(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jbyteArray raw_data,
    jobjectArray output, jobjectArray diagnostic) {
    return encode_input_v1_jni(env, token, raw_input, raw_data, output, diagnostic);
}

static jint encode_mouse_jni(
    JNIEnv* env, jlong token, jlongArray raw_input, jobjectArray output,
    jobjectArray diagnostic) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, raw_input, kMouseWords, &words)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalMouseInputV1 input{};
        input.abi_version = static_cast<uint32_t>(words[0]);
        input.struct_size = static_cast<uint32_t>(words[1]);
        input.action = static_cast<uint32_t>(words[2]);
        input.button = static_cast<uint32_t>(words[3]);
        input.wheel = static_cast<uint32_t>(words[4]);
        input.buttons_held = static_cast<uint32_t>(words[5]);
        input.position_flags = static_cast<uint32_t>(words[6]);
        input.modifiers = static_cast<uint32_t>(words[7]);
        input.cell_column = static_cast<uint32_t>(words[8]);
        input.cell_row = static_cast<uint32_t>(words[9]);
        input.pixel_x = static_cast<uint32_t>(words[10]);
        input.pixel_y = static_cast<uint32_t>(words[11]);
        input.reserved[0] = static_cast<uint64_t>(words[12]);
        input.reserved[1] = static_cast<uint64_t>(words[13]);
        if (diagnostic != nullptr &&
            (!minimum_length(env, diagnostic, 1) || env->IsSameObject(output, diagnostic))) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        BufferOwner detail;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return diagnostic == nullptr
                ? tl_terminal_encode_mouse_input_v1(terminal, &input, buffer.out())
                : tl_terminal_encode_mouse_input_v1_with_diagnostic(
                    terminal, &input, buffer.out(), detail.out());
        });
        if (status != TL_STATUS_OK) {
            if (diagnostic != nullptr && detail.get().data != nullptr) {
                const uint32_t publication = publish_buffer(
                    env, diagnostic, &detail, TL_INPUT_DIAGNOSTIC_V1_MAX_BYTES);
                if (publication != TL_STATUS_OK) {
                    return publication;
                }
            }
            return status;
        }
        return publish_buffer(env, output, &buffer, tl_terminal_host_effect_frame_v2_max_bytes());
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeMouse(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jobjectArray output) {
    return encode_mouse_jni(env, token, raw_input, output, nullptr);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeMouseWithDiagnostic(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jobjectArray output,
    jobjectArray diagnostic) {
    return encode_mouse_jni(env, token, raw_input, output, diagnostic);
}

static jint encode_input_v2_jni(
    JNIEnv* env, jlong token, jlongArray raw_input, jobjectArray output,
    jobjectArray diagnostic) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, raw_input, kInputV2Words, &words)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        if (words[6] != 0 || words[14] < 0 ||
            static_cast<uint64_t>(words[14]) > UINT32_MAX) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalInputV2 input{};
        input.abi_version = static_cast<uint32_t>(words[0]);
        input.struct_size = static_cast<uint32_t>(words[1]);
        input.kind = static_cast<uint32_t>(words[2]);
        input.key = static_cast<uint32_t>(words[3]);
        input.modifiers = static_cast<uint32_t>(words[4]);
        input.character = static_cast<uint32_t>(words[5]);
        input.data_len = static_cast<uint64_t>(words[7]);
        input.reserved_v1_compat[0] = static_cast<uint64_t>(words[8]);
        input.reserved_v1_compat[1] = static_cast<uint64_t>(words[9]);
        input.win32_virtual_key = static_cast<uint32_t>(words[10]);
        input.win32_scan_code = static_cast<uint32_t>(words[11]);
        input.win32_unicode_char = static_cast<uint32_t>(words[12]);
        input.win32_key_down = static_cast<uint32_t>(words[13]);
        input.win32_control_key_state = static_cast<uint32_t>(words[14]);
        input.win32_repeat_count = static_cast<uint32_t>(words[15]);
        input.reserved[0] = static_cast<uint64_t>(words[16]);
        input.reserved[1] = static_cast<uint64_t>(words[17]);
        if (diagnostic != nullptr &&
            (!minimum_length(env, diagnostic, 1) || env->IsSameObject(output, diagnostic))) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        BufferOwner detail;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return diagnostic == nullptr
                ? tl_terminal_encode_input_v2(terminal, &input, buffer.out())
                : tl_terminal_encode_input_v2_with_diagnostic(
                    terminal, &input, buffer.out(), detail.out());
        });
        if (status != TL_STATUS_OK) {
            if (diagnostic != nullptr && detail.get().data != nullptr) {
                const uint32_t publication = publish_buffer(
                    env, diagnostic, &detail, TL_INPUT_DIAGNOSTIC_V1_MAX_BYTES);
                if (publication != TL_STATUS_OK) {
                    return publication;
                }
            }
            return status;
        }
        return publish_buffer(env, output, &buffer, tl_terminal_host_effect_frame_v2_max_bytes());
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV2(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jobjectArray output) {
    return encode_input_v2_jni(env, token, raw_input, output, nullptr);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV2WithDiagnostic(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jobjectArray output,
    jobjectArray diagnostic) {
    return encode_input_v2_jni(env, token, raw_input, output, diagnostic);
}

static jint encode_input_v3_jni(
    JNIEnv* env, jlong token, jlongArray raw_input, jbyteArray associated_text,
    jobjectArray output, jobjectArray diagnostic) {
    return safe_status([&]() -> uint32_t {
        std::vector<jlong> words;
        if (!read_longs(env, raw_input, kInputV3Words, &words)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        ByteArrayPin text(env, associated_text);
        if (!text.valid()) {
            return TL_STATUS_PANIC;
        }
        if (words[6] != 0 || words[14] < 0 ||
            static_cast<uint64_t>(words[14]) > UINT32_MAX || words[22] < 0 ||
            static_cast<uint64_t>(words[22]) != text.size()) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalInputV3 input{};
        input.abi_version = static_cast<uint32_t>(words[0]);
        input.struct_size = static_cast<uint32_t>(words[1]);
        input.kind = static_cast<uint32_t>(words[2]);
        input.key = static_cast<uint32_t>(words[3]);
        input.modifiers = static_cast<uint32_t>(words[4]);
        input.character = static_cast<uint32_t>(words[5]);
        input.data_len = static_cast<uint64_t>(words[7]);
        input.reserved_v1_compat[0] = static_cast<uint64_t>(words[8]);
        input.reserved_v1_compat[1] = static_cast<uint64_t>(words[9]);
        input.win32_virtual_key = static_cast<uint32_t>(words[10]);
        input.win32_scan_code = static_cast<uint32_t>(words[11]);
        input.win32_unicode_char = static_cast<uint32_t>(words[12]);
        input.win32_key_down = static_cast<uint32_t>(words[13]);
        input.win32_control_key_state = static_cast<uint32_t>(words[14]);
        input.win32_repeat_count = static_cast<uint32_t>(words[15]);
        input.reserved[0] = static_cast<uint64_t>(words[16]);
        input.reserved[1] = static_cast<uint64_t>(words[17]);
        input.key_event_type = static_cast<uint32_t>(words[18]);
        input.keyboard_modifiers = static_cast<uint32_t>(words[19]);
        input.shifted_key = static_cast<uint32_t>(words[20]);
        input.base_layout_key = static_cast<uint32_t>(words[21]);
        input.associated_text = text.data();
        input.associated_text_len = text.size();
        input.reserved_v3[0] = static_cast<uint64_t>(words[23]);
        input.reserved_v3[1] = static_cast<uint64_t>(words[24]);
        input.reserved_v3[2] = static_cast<uint64_t>(words[25]);
        input.reserved_v3[3] = static_cast<uint64_t>(words[26]);
        if (diagnostic != nullptr &&
            (!minimum_length(env, diagnostic, 1) || env->IsSameObject(output, diagnostic))) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        BufferOwner detail;
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            return diagnostic == nullptr
                ? tl_terminal_encode_input_v3(terminal, &input, buffer.out())
                : tl_terminal_encode_input_v3_with_diagnostic(
                    terminal, &input, buffer.out(), detail.out());
        });
        if (status != TL_STATUS_OK) {
            if (diagnostic != nullptr && detail.get().data != nullptr) {
                const uint32_t publication = publish_buffer(
                    env, diagnostic, &detail, TL_INPUT_DIAGNOSTIC_V1_MAX_BYTES);
                if (publication != TL_STATUS_OK) {
                    return publication;
                }
            }
            return status;
        }
        return publish_buffer(env, output, &buffer, tl_terminal_host_effect_frame_v2_max_bytes());
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV3(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jbyteArray associated_text,
    jobjectArray output) {
    return encode_input_v3_jni(env, token, raw_input, associated_text, output, nullptr);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV3WithDiagnostic(
    JNIEnv* env, jobject, jlong token, jlongArray raw_input, jbyteArray associated_text,
    jobjectArray output, jobjectArray diagnostic) {
    return encode_input_v3_jni(env, token, raw_input, associated_text, output, diagnostic);
}

static jint export_snapshot_profile(
    JNIEnv* env, jlong token, jlong maximum, jlongArray output_info,
    jobjectArray output_payload, bool graphics) {
    return safe_status([&]() -> uint32_t {
        const uint64_t limit = graphics ? TL_GRAPHICS_SNAPSHOT_MAX_BYTES : kMaximumSnapshotBytes;
        if (maximum <= 0 || static_cast<uint64_t>(maximum) > limit ||
            !minimum_length(env, output_info, 2)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        BufferOwner buffer;
        TlTerminalCheckpointInfo info{};
        const uint32_t status = with_terminal(token, [&](TlTerminal* terminal) {
            CheckpointOwner checkpoint;
            uint32_t inner = checkpoint_for(terminal, &checkpoint, &info);
            if (inner == TL_STATUS_OK) {
                inner = graphics ? tl_terminal_checkpoint_export_snapshot_graphics_v1(checkpoint.get(), buffer.out())
                                 : tl_terminal_checkpoint_export_snapshot(checkpoint.get(), buffer.out());
            }
            return inner;
        });
        if (status != TL_STATUS_OK) {
            return status;
        }
        if (buffer.get().len > static_cast<uint64_t>(maximum)) {
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        if (graphics && (info.revision > INT64_MAX || info.next_source_sequence > INT64_MAX)) {
            return TL_STATUS_LIMIT_EXCEEDED;
        }
        const std::vector<jlong> values{static_cast<jlong>(info.revision),
                                        static_cast<jlong>(info.next_source_sequence)};
        if (!write_longs(env, output_info, values)) {
            return TL_STATUS_PANIC;
        }
        return publish_buffer(env, output_payload, &buffer, static_cast<uint64_t>(maximum));
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_exportCheckpoint(
    JNIEnv* env, jobject, jlong token, jlong maximum, jlongArray output_info,
    jobjectArray output_payload) {
    return export_snapshot_profile(env, token, maximum, output_info, output_payload, false);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_exportCheckpointGraphicsV1(
    JNIEnv* env, jobject, jlong token, jlong maximum, jlongArray output_info,
    jobjectArray output_payload) {
    return export_snapshot_profile(env, token, maximum, output_info, output_payload, true);
}

// Snapshot byte[] APIs keep context and detached apply explicit without exposing native pointers.
static uint32_t delta_context(JNIEnv* env, jbyteArray raw, jint profile,
                              TlTerminalDeltaContextV1* out) {
    if (!exact_length(env, raw, 32)) return TL_STATUS_INVALID_ARGUMENT;
    if (profile < static_cast<jint>(TL_STATE_DELTA_PROFILE_V1) ||
        profile > static_cast<jint>(TL_STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1))
        return TL_STATUS_INCOMPATIBLE_DELTA;
    out->abi_version = TL_TERMINAL_ABI_VERSION;
    out->struct_size = sizeof(*out);
    out->profile = static_cast<uint32_t>(profile);
    env->GetByteArrayRegion(raw, 0, 32, reinterpret_cast<jbyte*>(out->id));
    if (env->ExceptionCheck()) return TL_STATUS_PANIC;
    bool empty = true;
    for (uint8_t value : out->id) empty &= value == 0;
    return empty ? TL_STATUS_INCOMPATIBLE_DELTA : TL_STATUS_OK;
}

static uint32_t delta_checkpoint(JNIEnv* env, jbyteArray snapshot, TerminalOwner* terminal,
                                 CheckpointOwner* checkpoint) {
    if (snapshot == nullptr) return TL_STATUS_NULL_POINTER;
    if (env->GetArrayLength(snapshot) == 0 ||
        static_cast<uint64_t>(env->GetArrayLength(snapshot)) > TL_GRAPHICS_SNAPSHOT_MAX_BYTES) {
        return TL_STATUS_LIMIT_EXCEEDED;
    }
    TlTerminal* handle = nullptr;
    {
        ByteArrayPin pin(env, snapshot);
        if (!pin.valid()) return TL_STATUS_PANIC;
        const uint32_t status = tl_terminal_import_snapshot_graphics_v1(pin.data(), pin.size(), &handle);
        if (status != TL_STATUS_OK) return status;
    }
    terminal->reset(handle);
    return checkpoint_for(terminal->get(), checkpoint, nullptr);
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_deltaExportV1(
    JNIEnv* env, jobject, jbyteArray base_snapshot, jbyteArray target_snapshot,
    jbyteArray context_id, jint profile, jlong maximum, jobjectArray output_delta) {
    return safe_status([&]() -> uint32_t {
        if (maximum <= 0 || static_cast<uint64_t>(maximum) > TL_STATE_DELTA_MAX_BYTES ||
            !minimum_length(env, output_delta, 1)) return TL_STATUS_INVALID_ARGUMENT;
        TlTerminalDeltaContextV1 context{};
        uint32_t status = delta_context(env, context_id, profile, &context);
        if (status != TL_STATUS_OK) return status;
        TerminalOwner base_terminal, target_terminal;
        CheckpointOwner base, target;
        status = delta_checkpoint(env, base_snapshot, &base_terminal, &base);
        if (status != TL_STATUS_OK) return status;
        status = delta_checkpoint(env, target_snapshot, &target_terminal, &target);
        if (status != TL_STATUS_OK) return status;
        BufferOwner buffer;
        status = tl_terminal_checkpoint_export_delta_v1(
            base.get(), target.get(), &context, static_cast<uint64_t>(maximum), buffer.out());
        return status == TL_STATUS_OK
            ? publish_buffer(env, output_delta, &buffer, static_cast<uint64_t>(maximum)) : status;
    });
}

extern "C" JNIEXPORT jint JNICALL
Java_dev_termlink_terminal_nativecore_JniNativeMethods_deltaApplyV1(
    JNIEnv* env, jobject, jbyteArray current_snapshot, jbyteArray context_id,
    jint profile, jbyteArray delta, jlong maximum, jlongArray output_info, jobjectArray output_snapshot,
    jbyteArray output_sha256) {
    return safe_status([&]() -> uint32_t {
        if (maximum <= 0 || static_cast<uint64_t>(maximum) > TL_STATE_DELTA_MAX_BYTES ||
            !minimum_length(env, output_info, 3) || !minimum_length(env, output_snapshot, 1) ||
            !exact_length(env, output_sha256, 32) ||
            delta == nullptr || env->GetArrayLength(delta) == 0 ||
            static_cast<uint64_t>(env->GetArrayLength(delta)) > static_cast<uint64_t>(maximum)) {
            return TL_STATUS_INVALID_ARGUMENT;
        }
        TlTerminalDeltaContextV1 context{};
        uint32_t status = delta_context(env, context_id, profile, &context);
        if (status != TL_STATUS_OK) return status;
        TerminalOwner current_terminal;
        CheckpointOwner current;
        status = delta_checkpoint(env, current_snapshot, &current_terminal, &current);
        if (status != TL_STATUS_OK) return status;
        TlTerminal* detached = nullptr;
        TlTerminalDeltaApplyInfoV1 info{};
        info.abi_version = TL_TERMINAL_ABI_VERSION;
        info.struct_size = sizeof(info);
        {
            ByteArrayPin pin(env, delta);
            if (!pin.valid()) return TL_STATUS_PANIC;
            status = tl_terminal_checkpoint_apply_delta_v1(
                current.get(), &context, pin.data(), pin.size(), static_cast<uint64_t>(maximum),
                &detached, &info);
            if (status != TL_STATUS_OK) return status;
        }
        TerminalOwner target(detached);
        if (info.state.next_source_sequence > static_cast<uint64_t>(INT64_MAX)) return TL_STATUS_LIMIT_EXCEEDED;
        if (info.outcome != TL_DELTA_APPLIED && info.outcome != TL_DELTA_ALREADY_APPLIED)
            return TL_STATUS_PANIC;
        if ((info.outcome == TL_DELTA_APPLIED) != (target != nullptr)) return TL_STATUS_PANIC;
        BufferOwner buffer;
        if (target != nullptr) {
            CheckpointOwner checkpoint;
            status = checkpoint_for(target.get(), &checkpoint, nullptr);
            if (status != TL_STATUS_OK) return status;
            status = tl_terminal_checkpoint_export_snapshot_graphics_v1(checkpoint.get(), buffer.out());
            if (status != TL_STATUS_OK) return status;
        }
        // Create all Java output objects before mutating either output slot.
        jbyteArray bytes = nullptr;
        if (target != nullptr) {
            if (buffer.get().data == nullptr || buffer.get().len > TL_GRAPHICS_SNAPSHOT_MAX_BYTES ||
                buffer.get().len > static_cast<uint64_t>(std::numeric_limits<jsize>::max())) return TL_STATUS_PANIC;
            bytes = env->NewByteArray(static_cast<jsize>(buffer.get().len));
            if (bytes == nullptr || env->ExceptionCheck()) return TL_STATUS_PANIC;
            env->SetByteArrayRegion(bytes, 0, static_cast<jsize>(buffer.get().len),
                                    reinterpret_cast<const jbyte*>(buffer.get().data));
        } else {
            const jsize length = env->GetArrayLength(current_snapshot);
            bytes = env->NewByteArray(length);
            if (bytes == nullptr || env->ExceptionCheck()) return TL_STATUS_PANIC;
            ByteArrayPin pin(env, current_snapshot);
            if (!pin.valid()) { env->DeleteLocalRef(bytes); return TL_STATUS_PANIC; }
            env->SetByteArrayRegion(bytes, 0, length, reinterpret_cast<const jbyte*>(pin.data()));
        }
        if (env->ExceptionCheck()) { env->DeleteLocalRef(bytes); return TL_STATUS_PANIC; }
        const std::vector<jlong> values{static_cast<jlong>(info.outcome),
                                        static_cast<jlong>(info.state.next_source_sequence),
                                        static_cast<jlong>(info.state.snapshot_bytes)};
        env->SetObjectArrayElement(output_snapshot, 0, bytes);
        if (!env->ExceptionCheck()) env->SetLongArrayRegion(output_info, 0, 3, values.data());
        if (!env->ExceptionCheck()) env->SetByteArrayRegion(output_sha256, 0, 32,
            reinterpret_cast<const jbyte*>(info.state.sha256));
        env->DeleteLocalRef(bytes);
        return env->ExceptionCheck() ? TL_STATUS_PANIC : TL_STATUS_OK;
    });
}

extern "C" __attribute__((visibility("default"))) jint JNICALL JNI_OnLoad(JavaVM* vm, void*) {
    JNIEnv* env = nullptr;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK || env == nullptr) {
        return JNI_ERR;
    }
    jclass methods_class =
        env->FindClass("dev/termlink/terminal/nativecore/JniNativeMethods");
    if (methods_class == nullptr) {
        return JNI_ERR;
    }
    JNINativeMethod methods[] = {
        {const_cast<char*>("abiVersion"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_abiVersion)},
        {const_cast<char*>("renderFrameV1FormatVersion"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV1FormatVersion)},
        {const_cast<char*>("renderFrameV1MaxBytes"), const_cast<char*>("()J"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV1MaxBytes)},
        {const_cast<char*>("renderFrameV3FormatVersion"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV3FormatVersion)},
        {const_cast<char*>("renderFrameV3MaxBytes"), const_cast<char*>("()J"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV3MaxBytes)},
        {const_cast<char*>("hostEffectFrameV2FormatVersion"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_hostEffectFrameV2FormatVersion)},
        {const_cast<char*>("hostEffectFrameV2MaxBytes"), const_cast<char*>("()J"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_hostEffectFrameV2MaxBytes)},
        {const_cast<char*>("mouseInputV1StructSize"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_mouseInputV1StructSize)},
        {const_cast<char*>("inputV1StructSize"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_inputV1StructSize)},
        {const_cast<char*>("inputV2StructSize"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_inputV2StructSize)},
        {const_cast<char*>("inputV3StructSize"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_inputV3StructSize)},
        {const_cast<char*>("configV3StructSize"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_configV3StructSize)},
        {const_cast<char*>("create"), const_cast<char*>("(IIJ[J)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_create)},
        {const_cast<char*>("createV3"), const_cast<char*>("([J[J)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_createV3)},
        {const_cast<char*>("importCheckpoint"), const_cast<char*>("([B[J)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_importCheckpoint)},
        {const_cast<char*>("destroy"), const_cast<char*>("(J)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_destroy)},
        {const_cast<char*>("applyBatch"), const_cast<char*>("(J[J[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBatch)},
        {const_cast<char*>("applyBatchWithHostEffectsV2"),
         const_cast<char*>("(J[J[[B[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBatchWithHostEffectsV2)},
        {const_cast<char*>("render"), const_cast<char*>("(JJ[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_render)},
        {const_cast<char*>("renderV3"), const_cast<char*>("(JJ[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV3)},
        {const_cast<char*>("renderV3AtScrollback"), const_cast<char*>("(JJJ[J[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV3AtScrollback)},
        {const_cast<char*>("encodeInputV1"), const_cast<char*>("(J[J[B[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV1)},
        {const_cast<char*>("encodeMouse"), const_cast<char*>("(J[J[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeMouse)},
        {const_cast<char*>("encodeInputV2"), const_cast<char*>("(J[J[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV2)},
        {const_cast<char*>("encodeInputV3"), const_cast<char*>("(J[J[B[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV3)},
        {const_cast<char*>("exportCheckpoint"), const_cast<char*>("(JJ[J[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_exportCheckpoint)},
    };
    const jint registration = env->RegisterNatives(
        methods_class, methods, static_cast<jint>(sizeof(methods) / sizeof(methods[0])));
    if (registration == JNI_OK) {
        // Older bindings need not declare additive APIs.
        const JNINativeMethod optional_methods[] = {
            {const_cast<char*>("encodeInputV1WithDiagnostic"), const_cast<char*>("(J[J[B[[B[[B)I"),
             reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV1WithDiagnostic)},
            {const_cast<char*>("encodeMouseWithDiagnostic"), const_cast<char*>("(J[J[[B[[B)I"),
             reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeMouseWithDiagnostic)},
            {const_cast<char*>("encodeInputV2WithDiagnostic"), const_cast<char*>("(J[J[[B[[B)I"),
             reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV2WithDiagnostic)},
            {const_cast<char*>("encodeInputV3WithDiagnostic"), const_cast<char*>("(J[J[B[[B[[B)I"),
             reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_encodeInputV3WithDiagnostic)},
            {const_cast<char*>("applyBrokerUnsupportedV1"), const_cast<char*>("(JJ[B[[B[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBrokerUnsupportedV1)},
            {const_cast<char*>("applyBrokerCapturedV1"), const_cast<char*>("(JJ[B[[B[I[[B[[B[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_applyBrokerCapturedV1)},
            {const_cast<char*>("deltaExportV1"), const_cast<char*>("([B[B[BIJ[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_deltaExportV1)},
            {const_cast<char*>("deltaApplyV1"), const_cast<char*>("([B[BI[BJ[J[[B[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_deltaApplyV1)},
            {const_cast<char*>("createGraphicsV1"), const_cast<char*>("([J[J)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_createGraphicsV1)},
            {const_cast<char*>("importSnapshotGraphicsV1"), const_cast<char*>("([B[J)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_importSnapshotGraphicsV1)},
            {const_cast<char*>("exportCheckpointGraphicsV1"), const_cast<char*>("(JJ[J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_exportCheckpointGraphicsV1)},
            {const_cast<char*>("maxGraphicsSnapshotBytes"), const_cast<char*>("()J"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_maxGraphicsSnapshotBytes)},
            {const_cast<char*>("renderFrameV5FormatVersion"), const_cast<char*>("()I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV5FormatVersion)},
            {const_cast<char*>("renderFrameV5MaxBytes"), const_cast<char*>("()J"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV5MaxBytes)},
            {const_cast<char*>("renderV5"), const_cast<char*>("(JI[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV5)},
            {const_cast<char*>("renderV5AtScrollback"), const_cast<char*>("(JJI[J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV5AtScrollback)},
            {const_cast<char*>("historyPageV2"), const_cast<char*>("([J[J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyPageV2)},
            {const_cast<char*>("historyViewportV2"), const_cast<char*>("(JJJJ[J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyViewportV2)},
            {const_cast<char*>("hostSignalsV1"), const_cast<char*>("(J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_hostSignalsV1)},
            {const_cast<char*>("historyResolveAnchor"), const_cast<char*>("(JJJJJ[J)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyResolveAnchor)},
            {const_cast<char*>("shellStateV1"), const_cast<char*>("(JJ[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_shellStateV1)},
            {const_cast<char*>("historyBegin"), const_cast<char*>("(J[J)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyBegin)},
            {const_cast<char*>("historyViewport"), const_cast<char*>("(JJJJ[J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyViewport)},
            {const_cast<char*>("historyPage"), const_cast<char*>("([J[J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyPage)},
            {const_cast<char*>("historyRelease"), const_cast<char*>("(JJ)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_historyRelease)},
            {const_cast<char*>("renderV4AtScrollback"), const_cast<char*>("(JJJ[J[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV4AtScrollback)},
            {const_cast<char*>("renderV4"), const_cast<char*>("(JJ[[B)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderV4)},
            {const_cast<char*>("createV4"), const_cast<char*>("([J[J)I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_createV4)},
            {const_cast<char*>("configV4StructSize"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_configV4StructSize)},
            {const_cast<char*>("renderFrameV4MaxBytes"), const_cast<char*>("()J"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV4MaxBytes)},
            {const_cast<char*>("renderFrameV4FormatVersion"), const_cast<char*>("()I"),
         reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_renderFrameV4FormatVersion)},
            {const_cast<char*>("failureDiagnosticV1"), const_cast<char*>("(J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_failureDiagnosticV1)},
            {const_cast<char*>("failureDiagnosticV2"), const_cast<char*>("(J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_failureDiagnosticV2)},
            {const_cast<char*>("failureDiagnosticV3"), const_cast<char*>("(J[[B)I"), reinterpret_cast<void*>(Java_dev_termlink_terminal_nativecore_JniNativeMethods_failureDiagnosticV3)},
        };
        for (const auto& diagnostic : optional_methods) {
            const jmethodID diagnostic_method = env->GetMethodID(methods_class, diagnostic.name, diagnostic.signature);
            if (diagnostic_method != nullptr) {
                if (env->RegisterNatives(methods_class, &diagnostic, 1) != JNI_OK) {
                    env->DeleteLocalRef(methods_class);
                    return JNI_ERR;
                }
            } else {
                const jthrowable error = env->ExceptionOccurred();
                env->ExceptionClear();
                const jclass missing_method = env->FindClass("java/lang/NoSuchMethodError");
                const bool optional_missing = error != nullptr && missing_method != nullptr &&
                    env->IsInstanceOf(error, missing_method);
                if (missing_method != nullptr) env->DeleteLocalRef(missing_method);
                if (error != nullptr) env->DeleteLocalRef(error);
                if (!optional_missing) {
                    env->DeleteLocalRef(methods_class);
                    return JNI_ERR;
                }
            }
        }
    }
    env->DeleteLocalRef(methods_class);
    return registration == JNI_OK ? JNI_VERSION_1_6 : JNI_ERR;
}

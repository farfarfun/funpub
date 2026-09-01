"""funpub 统一异常定义"""


class PublishError(Exception):
    """funpub 基础异常"""


class AuthenticationError(PublishError):
    """认证失败，如账号密码错误或缺失"""


class RemoteNotFoundError(PublishError):
    """远端制品不存在"""


class ChecksumMismatchError(PublishError):
    """本地计算的校验和与远端返回值不一致"""


class MergeTimeoutError(PublishError):
    """分块上传合并轮询超时"""


class ChannelNotFoundError(PublishError):
    """请求了未注册的发布渠道"""


__all__ = [
    "PublishError",
    "AuthenticationError",
    "RemoteNotFoundError",
    "ChecksumMismatchError",
    "MergeTimeoutError",
    "ChannelNotFoundError",
]

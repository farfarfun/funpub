API上传和下载
通过本指南介绍如何通过API进行制品上传和下载。

API上传制品：
请求路径：

POST https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/{filePath:**}?version={version}&fileName={fileName}&versionDescription={versionDescription}
请求参数:

filePath:制品文件路径（注意路径中不能出现&、?、空格等特殊字符）
version:上传版本
fileName:制品名称（可选）
versionDescription:制品描述（可选）
file: 制品文件
API采用basic认证方式进行鉴权，账号密码如下：

账号
用户名: {username}
密码: {password}
示例：

curl -u '{username}:{password}' -XPOST 'https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/a/b/c?version=1.2.3&fileName=output.tgz&versionDescription=logfile' -F file=@output.tgz
响应示例:

{
  "object": {
    "fileMd5": "c7ac13f3ba18f4a37a871640bd435b2c",  //文件md5信息
    "fileSha1": "c1ee54a3e49a69957c45eb984acab76d99666870",    //文件sha1信息
    "fileSha256": "e10e1bcfe3941036efc96f5ca660e8dd627a69ca7d47eff72fcb6194f1ecd90d",  //文件sha256信息
    "fileSize": 320,  //文件大小（单位字节）
    "url": "https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/a/b/c/output.tgz?version=1.2.3&userId={userId}&expiration={expiration}&signature={signature}" //临时免密下载地址
  },
  "successful": true
}

API下载制品：
GET https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/{filePath:**}?version={version}
请求参数：

filePath: 文件路径（包含文件名称)，即制品列表页中的包名。
version: 制品版本
API采用basic认证方式进行鉴权，账号密码如下：

账号
用户名: {username}
密码: {password}
示例：

curl -u '{username}:{password}' 'https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/a/b/c/output.tgz?version=1.2.3' --output download.tgz
API获取临时免密下载地址：
某些场景下用户需要生成一个临时免密下载地址，将这个地址分享给别人或者传递给其他系统进行下载。以下是获取临时免密下载地址的API：

HEAD https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/{filePath:**}?version={version}&signUrl=true&expiration={expiration}
请求参数：

filePath: 文件路径（包含文件名称)，即制品列表页中的包名。
version: 制品版本
signUrl: true, 表示生成临时免密下载地址
expiration: 临时免密下载地址的过期时间，毫秒级时间戳。
免密下载地址在response header中，key为x-artlab-generic-sign-url。以下是返回的header介绍。

x-artlab-generic-sign-url: 临时免密下载地址
x-artlab-checksum-sha1: 文件sha1信息
x-artlab-checksum-md5: 文件md5信息
x-artlab-checksum-sha256: 文件sha256信息
X-artlab-generic-version-description: 版本描述信息
示例：

curl -u '{username}:{password}' -I 'https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/a/b/c/output.tgz?version=1.2.3&signUrl=true&expiration={expiration}'
以下是返回的header示例。

date: Fri, 13 Jun 2025 03:30:19 GMT
content-type: application/octet-stream
content-length: 0
set-cookie: XSRF-TOKEN={xsrf_token}; Path=/
x-csp-nonce: {nonce}
content-security-policy-report-only: base-uri 'self';script-src 'self' 'unsafe-inline' 'unsafe-eval' 'report-sample' https: http: 'nonce-{nonce}' 'Strict-Dynamic' 'sha256-lfXlPY3+MCPOPb4mrw1Y961+745U3WlDQVcOXdchSQc=' 'unsafe-hashes';frame-src 'self' *.aliyun.com gaic.alicdn.com g.alicdn.com;worker-src blob: 'self' data:;object-src 'self' g.alicdn.com;frame-ancestors *.aliyun.com;report-uri /data/report-csp;
set-cookie: grayTraffic=; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:10 GMT; Path=/
set-cookie: grayTraffic=; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:10 GMT
etag: "d404401c8c6495b206fc35c95e55a6d5"
x-artlab-checksum-sha1: 312382290f4f71e7fb7f00449fb529fce3b8ec95    //文件sha1信息
x-artlab-checksum-sha256: d9cd8155764c3543f10fad8a480d743137466f8d55213c8eaefcd12f06d43a80 //文件sha56信息
x-artlab-checksum-md5: d404401c8c6495b206fc35c95e55a6d5   //文件MD5信息
artlab-api-version: artlab/1.0
content-disposition: attachment; filename="output.tgz"
x-artlab-generic-sign-url: https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}/files/a/b/c/output.tgz?version=1.2.3&userId={userId}&expiration={expiration}&signature={signature}   //免密下载地址
x-artlab-generic-version-description: logfile  //描述信息
x-envoy-upstream-service-time: 50
x-forwarded-for: {client_ip}
用户可以通过x-artlab-generic-sign-url获取的临时免密下载地址直接进行文件下载。
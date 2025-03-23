# Dirmap

`Dirmap` 是一个扫描指定目录生成扁平的目录结构的工具, 扫描后将序列化为 `bincode` 格式, 通过 `zstd` 压缩存储在 map 文件中

实机测试:

```sh
❯ time .\target\release\dirmap.exe .
real    0m 0.02s
user    0m 0.01s
sys     0m 0.00s

❯ ls -al
...
.a--- ? ?  17 KB Sat Mar 22 17:46:22 2025  map
...
```

测试目录大小: 258MiB
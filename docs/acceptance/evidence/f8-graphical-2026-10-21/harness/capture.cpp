#include <QCoreApplication>
#include <QDBusInterface>
#include <QDBusReply>
#include <QDBusUnixFileDescriptor>
#include <QImage>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QDir>
#include <thread>
#include <vector>
#include <unistd.h>
#include <cstdio>
int main(int argc, char **argv) {
 QCoreApplication app(argc,argv);
 if(argc!=3)return 2;
 const QString root="/tmp/wyss-f8v-sb_mv5xg";
 QFile f(root+"/pids.json");if(!f.open(QIODevice::ReadOnly))return 3;
 const int pid=QJsonDocument::fromJson(f.readAll()).object()["gui"].toInt();
 const QString id=QString::fromLocal8Bit(argv[1]);
 const QString output=QString::fromLocal8Bit(argv[2]);
 if(QFileInfo(output).absolutePath()!=root)return 4;
 QDBusInterface kwin("org.kde.KWin","/KWin","org.kde.KWin");
 QDBusReply<QVariantMap> info=kwin.call("getWindowInfo",id);
 if(!info.isValid()||info.value()["pid"].toInt()!=pid)return 5;
 int p[2];if(pipe(p))return 6;
 std::vector<unsigned char> raw;
 std::thread reader([&]{unsigned char b[65536];ssize_t n;while((n=read(p[0],b,sizeof b))>0){raw.insert(raw.end(),b,b+n);if(raw.size()>128*1024*1024)break;}close(p[0]);});
 QDBusInterface shot("org.kde.KWin","/org/kde/KWin/ScreenShot2","org.kde.KWin.ScreenShot2");
 QDBusReply<QVariantMap> reply=shot.call("CaptureWindow",id,QVariantMap{{"include-decoration",true}},QVariant::fromValue(QDBusUnixFileDescriptor(p[1])));
 close(p[1]);reader.join();
 if(!reply.isValid()){fprintf(stderr,"%s\n",qPrintable(reply.error().message()));return 7;}
 auto m=reply.value();const int w=m["width"].toInt(),h=m["height"].toInt(),stride=m["stride"].toInt(),format=m["format"].toInt();
 if(w<=0||h<=0||stride<w*4||raw.size()!=size_t(stride)*h)return 8;
 QImage image(raw.data(),w,h,stride,QImage::Format(format));
 if(!image.save(output))return 9;
 printf("captured PID %d, %dx%d, format %d, %zu bytes\n",pid,w,h,format,raw.size());
}
